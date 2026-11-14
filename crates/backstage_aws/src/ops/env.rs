//! Per-namespace environments: up, down, list, status.
use crate::{
    ctx::{Ctx, StepTiming, poll, step, write_summary},
    db,
    error::{Error, Result},
    github,
    terraform::{self, Stack, output_str},
};
use aws_sdk_ecs::types::{DeploymentRolloutState, Service};
use aws_sdk_elasticloadbalancingv2::types::TargetHealthStateEnum;
use serde::Serialize;
use serde_json::Value;
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// Fifteen minutes of polling for a rollout, five for target health.
const ROLLOUT_POLLS: u32 = 300;
const HEALTH_POLLS: u32 = 100;
const MAX_LIST_PAGES: u32 = 1000;
/// `terraform destroy` validates its variables but never deploys the image.
const NO_IMAGE: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Serialize)]
pub(crate) struct EnvUpResult {
    namespace: String,
    url: String,
    image_digest: String,
    timings: Vec<StepTiming>,
    /// The pull-request comment announcing the environment, when one was written.
    comment_url: Option<String>,
}
#[derive(Serialize)]
pub(crate) struct EnvDownResult {
    namespace: String,
    destroyed: bool,
    comment_url: Option<String>,
}
#[derive(Serialize)]
pub(crate) struct EnvListResult {
    namespaces: Vec<String>,
}
#[derive(Serialize)]
pub(crate) struct EnvStatusResult {
    namespace: String,
    url: String,
    service_status: String,
    running_count: i32,
    image_digest: String,
    ready: bool,
}

pub(crate) async fn env_up(ctx: &Ctx, digest: Option<&str>, scan: bool) -> Result<EnvUpResult> {
    ctx.require_approval()?;
    let started = Instant::now();
    let mut timings = Vec::new();
    // The image and the database do not depend on each other, so the build
    // and the clone task run at the same time.
    let image = async {
        match digest {
            Some(digest) => super::build::validate_digest(digest).map(|()| digest.to_owned()),
            None => Ok(super::build(ctx, None).await?.image_digest),
        }
    };
    let (image, database) = tokio::join!(
        step("image.build", image),
        step("database.clone", db::clone_database(ctx))
    );
    let (image, build_timing) = image?;
    timings.extend([build_timing, database?.1]);
    if scan {
        timings.push(step("image.scan", super::scan(ctx, &image)).await?.1);
    }
    let vars = ctx.settings.env_tfvars(&image);
    let apply = terraform::apply(ctx, Stack::Env, &vars);
    timings.push(step("terraform.apply", apply).await?.1);
    let outputs = terraform::required_outputs(ctx, Stack::Env).await?;
    timings.push(step("ecs.rollout", wait_rollout(ctx, &outputs)).await?.1);
    timings.push(step("alb.healthy", wait_healthy(ctx, &outputs)).await?.1);
    write_summary(&timings)?;
    let url = output_str(&outputs, "url")?.to_owned();
    let seconds = started.elapsed().as_secs();
    let text = format!(
        "### Your on-demand environment is ready\n\n**{url}**\n\n\
         | | |\n|---|---|\n| Namespace | `{}` |\n| Image | `{image}` |\n| Ready in | {} min {} s |\n\n\
         Every push to this pull request redeploys it. Closing the pull request removes it.",
        ctx.settings.namespace,
        seconds / 60,
        seconds % 60,
    );
    Ok(EnvUpResult {
        comment_url: github::comment(ctx, &text).await?,
        namespace: ctx.settings.namespace.clone(),
        url,
        image_digest: image,
        timings,
    })
}

/// Removes the environment. Every step is safe to repeat, so an interrupted
/// teardown is finished by running the command again.
pub(crate) async fn env_down(ctx: &Ctx) -> Result<EnvDownResult> {
    ctx.require_approval()?;
    let state = terraform::outputs(ctx, Stack::Env).await?;
    if let Some(outputs) = &state {
        // Destroy first so no task is connected when the database is dropped.
        let image = outputs.get("image_digest").and_then(Value::as_str);
        let vars = ctx.settings.env_tfvars(image.unwrap_or(NO_IMAGE));
        terraform::destroy(ctx, Stack::Env, &vars).await?;
    }
    db::drop_database(ctx).await?;
    ctx.s3
        .delete_object()
        .bucket(ctx.settings.state_bucket())
        .key(ctx.settings.env_state_key())
        .send()
        .await
        .map_err(|error| Error::aws_failed("s3", "DeleteObject", error.to_string()))?;
    let namespace = ctx.settings.namespace.clone();
    let text = format!(
        "### On-demand environment removed\n\n`{namespace}`, its database, and its Terraform state are deleted."
    );
    Ok(EnvDownResult {
        comment_url: github::comment(ctx, &text).await?,
        namespace,
        destroyed: state.is_some(),
    })
}

pub(crate) async fn env_list(ctx: &Ctx) -> Result<EnvListResult> {
    let mut pages = ctx
        .s3
        .list_objects_v2()
        .bucket(ctx.settings.state_bucket())
        .prefix("env/")
        .into_paginator()
        .send();
    let mut namespaces = Vec::new();
    for _ in 0..MAX_LIST_PAGES {
        let Some(page) = pages.next().await else {
            return Ok(EnvListResult { namespaces });
        };
        let page =
            page.map_err(|error| Error::aws_failed("s3", "ListObjectsV2", error.to_string()))?;
        namespaces.extend(page.contents().iter().filter_map(|object| {
            let name = object
                .key()?
                .strip_prefix("env/")?
                .strip_suffix(".tfstate")?;
            Some(name.to_owned())
        }));
    }
    Err(Error::precondition_failed(
        "more environment state objects than this command lists",
    ))
}

pub(crate) async fn env_status(ctx: &Ctx) -> Result<EnvStatusResult> {
    let outputs = terraform::required_outputs(ctx, Stack::Env).await?;
    let service = describe_service(ctx, &outputs).await?;
    Ok(EnvStatusResult {
        namespace: ctx.settings.namespace.clone(),
        url: output_str(&outputs, "url")?.to_owned(),
        service_status: service.status().unwrap_or("UNKNOWN").to_owned(),
        running_count: service.running_count(),
        image_digest: output_str(&outputs, "image_digest")?.to_owned(),
        ready: has_healthy_target(ctx, &outputs).await?,
    })
}

async fn describe_service(ctx: &Ctx, outputs: &Value) -> Result<Service> {
    let response = ctx
        .ecs
        .describe_services()
        .cluster(output_str(outputs, "ecs_cluster_name")?)
        .services(output_str(outputs, "ecs_service_name")?)
        .send()
        .await
        .map_err(|error| Error::aws_failed("ecs", "DescribeServices", error.to_string()))?;
    let service = response.services().first().cloned();
    service.ok_or_else(|| Error::precondition_failed("the environment's ECS service is missing"))
}

async fn wait_rollout(ctx: &Ctx, outputs: &Value) -> Result<()> {
    let expected = output_str(outputs, "task_definition_arn")?;
    let service = output_str(outputs, "ecs_service_name")?;
    let probe = || async { rollout(&describe_service(ctx, outputs).await?, expected) };
    poll(
        "ECS deployment",
        service,
        POLL_INTERVAL,
        ROLLOUT_POLLS,
        probe,
    )
    .await
}

/// Whether the deployment of `expected` has finished. The deployment circuit
/// breaker rolls a failed deployment back by deploying the previous task
/// definition, so a different primary task definition is a failure too.
fn rollout(service: &Service, expected: &str) -> Result<Option<()>> {
    let deployments = service.deployments();
    let Some(primary) = deployments
        .iter()
        .find(|one| one.status() == Some("PRIMARY"))
    else {
        return Ok(None);
    };
    let failed = |reason: &str| {
        Error::aws_failed(
            "ecs",
            "deployment",
            format!("{reason}; see the service events and the task logs"),
        )
    };
    if primary.task_definition() != Some(expected) {
        return Err(failed("the deployment was rolled back"));
    }
    match primary.rollout_state() {
        Some(DeploymentRolloutState::Completed) => Ok(Some(())),
        Some(DeploymentRolloutState::Failed) => Err(failed(
            primary
                .rollout_state_reason()
                .unwrap_or("the deployment failed"),
        )),
        _ => Ok(None),
    }
}

/// Waits until the load balancer itself reports a healthy target. Its health
/// check is the readiness endpoint, and unlike this process it is always
/// allowed through the ingress allowlist.
async fn wait_healthy(ctx: &Ctx, outputs: &Value) -> Result<()> {
    let group = output_str(outputs, "target_group_arn")?;
    let probe = || async { Ok(has_healthy_target(ctx, outputs).await?.then_some(())) };
    poll(
        "load balancer health check",
        group,
        POLL_INTERVAL,
        HEALTH_POLLS,
        probe,
    )
    .await
}

async fn has_healthy_target(ctx: &Ctx, outputs: &Value) -> Result<bool> {
    let response = ctx
        .elb
        .describe_target_health()
        .target_group_arn(output_str(outputs, "target_group_arn")?)
        .send()
        .await
        .map_err(|error| {
            Error::aws_failed(
                "elasticloadbalancing",
                "DescribeTargetHealth",
                error.to_string(),
            )
        })?;
    let healthy =
        |state: Option<&TargetHealthStateEnum>| state == Some(&TargetHealthStateEnum::Healthy);
    let targets = response.target_health_descriptions();
    Ok(targets
        .iter()
        .any(|target| healthy(target.target_health().and_then(|health| health.state()))))
}

#[cfg(test)]
mod tests;
