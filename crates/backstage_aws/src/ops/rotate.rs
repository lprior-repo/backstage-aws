//! Rotates the application database password. The password exists only in
//! Secrets Manager request and response bodies and in this process's memory.
use crate::{
    ctx::{Ctx, StepTiming, step},
    db,
    error::{Error, Result},
    terraform::{self, Stack, output_str},
};
use aws_sdk_ecs::client::Waiters;
use serde::Serialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Serialize)]
pub(crate) struct RotationResult {
    secret_arn: String,
    version_id: String,
    services_restarted: Vec<String>,
    timings: Vec<StepTiming>,
}

/// The new password is staged as `AWSPENDING`, set on the database role by a
/// task that reads that stage, and only then made `AWSCURRENT`. A failure
/// before the last step leaves both the role and the current secret as they
/// were; after any failure, rerun the command.
pub(crate) async fn rotate_password(ctx: &Ctx) -> Result<RotationResult> {
    ctx.require_approval()?;
    let base = terraform::required_outputs(ctx, Stack::Base).await?;
    let secret_arn = output_str(&base, "database_application_secret_arn")?.to_owned();
    let ((version_id, replaced), staged) =
        step("secret.stage", stage_new_password(ctx, &secret_arn)).await?;
    let task = "db_admin_rotate_task_definition_arn";
    let alter = db::run_task(ctx, &base, task, Vec::new());
    let ((), altered) = step("database.alter-role", alter).await?;
    let promotion = ctx
        .secrets
        .update_secret_version_stage()
        .secret_id(&secret_arn)
        .version_stage("AWSCURRENT")
        .move_to_version_id(&version_id)
        .set_remove_from_version_id(replaced);
    promotion.send().await.map_err(|error| {
        Error::aws_failed(
            "secretsmanager",
            "UpdateSecretVersionStage",
            error.to_string(),
        )
    })?;
    let cluster = output_str(&base, "ecs_cluster_name")?;
    let (services_restarted, restarted) =
        Box::pin(step("services.restart", restart_services(ctx, cluster))).await?;
    Ok(RotationResult {
        secret_arn,
        version_id,
        services_restarted,
        timings: vec![staged, altered, restarted],
    })
}

/// Stages a copy of the secret's JSON document with a new `password`, keeping
/// its other keys (the task definitions read `username` and `password` from
/// it). Returns the staged version and the current one it will replace.
async fn stage_new_password(ctx: &Ctx, secret_arn: &str) -> Result<(String, Option<String>)> {
    let failed = |operation, detail: &str| Error::aws_failed("secretsmanager", operation, detail);
    let current = ctx
        .secrets
        .get_secret_value()
        .secret_id(secret_arn)
        .send()
        .await
        .map_err(|error| failed("GetSecretValue", &error.to_string()))?;
    let mut document: Value = current
        .secret_string()
        .and_then(|text| serde_json::from_str(text).ok())
        .filter(Value::is_object)
        .ok_or_else(|| failed("GetSecretValue", "the secret is not a JSON object"))?;
    let generated = ctx
        .secrets
        .get_random_password()
        .password_length(40)
        .exclude_punctuation(true)
        .send()
        .await
        .map_err(|error| failed("GetRandomPassword", &error.to_string()))?;
    let password = generated
        .random_password()
        .ok_or_else(|| failed("GetRandomPassword", "no password returned"))?;
    if let Some(fields) = document.as_object_mut() {
        fields.insert("password".to_owned(), Value::from(password));
    }
    // Error text from the service is not echoed here: it can quote the request.
    let stored = ctx
        .secrets
        .put_secret_value()
        .secret_id(secret_arn)
        .secret_string(document.to_string())
        .version_stages("AWSPENDING")
        .send()
        .await
        .map_err(|_| failed("PutSecretValue", "the new password was not stored"))?;
    let staged = stored.version_id().unwrap_or_default().to_owned();
    Ok((staged, current.version_id().map(str::to_owned)))
}

/// Starts a new deployment of every service in the cluster, then waits for all
/// of them, so each task picks up the new secret value.
async fn restart_services(ctx: &Ctx, cluster: &str) -> Result<Vec<String>> {
    let listed = ctx
        .ecs
        .list_services()
        .cluster(cluster)
        .max_results(100)
        .send()
        .await
        .map_err(|error| Error::aws_failed("ecs", "ListServices", error.to_string()))?;
    if listed.next_token().is_some() {
        return Err(Error::precondition_failed(
            "the cluster has more than 100 services; restart them in batches by hand",
        ));
    }
    let services = listed.service_arns();
    for service in services {
        ctx.ecs
            .update_service()
            .cluster(cluster)
            .service(service)
            .force_new_deployment(true)
            .send()
            .await
            .map_err(|error| Error::aws_failed("ecs", "UpdateService", error.to_string()))?;
    }
    // DescribeServices, which the waiter polls, accepts ten services a call.
    for batch in services.chunks(10) {
        ctx.ecs
            .wait_until_services_stable()
            .cluster(cluster)
            .set_services(Some(batch.to_vec()))
            .wait(Duration::from_mins(30))
            .await
            .map_err(|error| Error::waiter("service restart", cluster, error))?;
    }
    Ok(services.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_secretsmanager::{
        Client,
        operation::{
            get_random_password::GetRandomPasswordOutput, get_secret_value::GetSecretValueOutput,
            put_secret_value::PutSecretValueOutput,
        },
    };
    use aws_smithy_mocks::{Rule, RuleMode, mock, mock_client};
    use serde_json::json;

    const SECRET: &str = "arn:aws:secretsmanager:us-east-1:123456789012:secret:portal/db";

    fn current(document: &'static str) -> Rule {
        mock!(Client::get_secret_value).then_output(move || {
            GetSecretValueOutput::builder()
                .secret_string(document)
                .version_id("v1")
                .build()
        })
    }

    fn generated() -> Rule {
        mock!(Client::get_random_password).then_output(|| {
            GetRandomPasswordOutput::builder()
                .random_password("fresh")
                .build()
        })
    }

    /// Accepts only the document the task definitions can still read, and only
    /// as a pending version: tasks must not see it before the role has it.
    fn stored() -> Rule {
        mock!(Client::put_secret_value)
            .match_requests(|request| {
                let text = request.secret_string();
                let document = text.and_then(|text| serde_json::from_str::<Value>(text).ok());
                let expected = json!({"username": "backstage_app", "password": "fresh"});
                request.secret_id() == Some(SECRET)
                    && document == Some(expected)
                    && request.version_stages() == ["AWSPENDING"]
            })
            .then_output(|| PutSecretValueOutput::builder().version_id("v2").build())
    }

    type Versions = (String, Option<String>);

    async fn rotate(rules: &[&Rule]) -> std::result::Result<Versions, String> {
        let dir = std::env::temp_dir();
        let mut ctx = Ctx::sample(&dir, true).map_err(|error| error.to_string())?;
        ctx.secrets = mock_client!(aws_sdk_secretsmanager, RuleMode::MatchAny, rules);
        let staged = stage_new_password(&ctx, SECRET).await;
        staged.map_err(|error| error.to_string())
    }

    #[tokio::test]
    async fn rotation_stages_the_new_password_and_keeps_the_rest_of_the_document() {
        let document = current(r#"{"username":"backstage_app","password":"stale"}"#);
        let (generated, stored) = (generated(), stored());
        let versions = rotate(&[&document, &generated, &stored]).await;
        assert_eq!(versions, Ok(("v2".to_owned(), Some("v1".to_owned()))));
        assert_eq!(stored.num_calls(), 1);
    }

    #[tokio::test]
    async fn a_secret_that_is_not_a_json_object_is_never_overwritten() {
        for document in ["stale", r#""stale""#, "[]"] {
            let (generated, stored) = (generated(), stored());
            let refused = rotate(&[&current(document), &generated, &stored]).await;
            assert_eq!(
                refused,
                Err("secretsmanager GetSecretValue: the secret is not a JSON object".to_owned())
            );
            assert_eq!(stored.num_calls(), 0);
        }
    }
}
