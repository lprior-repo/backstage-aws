//! Whether an environment is ready, decided from AWS APIs alone, plus a
//! best-effort probe through the load balancer that never decides anything.
use aws_sdk_ecs::types::{DeploymentRolloutState, Service};
use aws_sdk_elasticloadbalancingv2::types::{TargetHealthDescription, TargetHealthStateEnum};
use serde::Serialize;
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const READINESS_PATH: &str = "/.backstage/health/v1/readiness";

/// The verdict from the ECS service and the target group.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Readiness {
    Ready,
    ServiceNotSteady,
    NoHealthyTarget,
}

/// The service is steady when it is active, runs exactly the desired number
/// of tasks and no deployment is still rolling out or has failed.
pub(super) fn service_steady(service: &Service) -> bool {
    let settled = service.deployments().iter().all(|one| {
        one.rollout_state()
            .is_none_or(|state| *state == DeploymentRolloutState::Completed)
    });
    service.status() == Some("ACTIVE")
        && service.desired_count() > 0
        && service.running_count() == service.desired_count()
        && settled
}

pub(super) fn any_healthy(targets: &[TargetHealthDescription]) -> bool {
    targets.iter().any(|target| {
        target.target_health().and_then(|health| health.state())
            == Some(&TargetHealthStateEnum::Healthy)
    })
}

pub(super) const fn decide(steady: bool, healthy_target: bool) -> Readiness {
    if !steady {
        Readiness::ServiceNotSteady
    } else if !healthy_target {
        Readiness::NoHealthyTarget
    } else {
        Readiness::Ready
    }
}

/// The outcome of the optional request through the load balancer.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(super) enum NetworkProbe {
    /// A domain serves the environment over HTTPS, so there is nothing to probe here.
    Skipped,
    /// The load balancer answered; `status` is the HTTP status code.
    Reachable { status: u16 },
    /// This machine is outside the ingress allow-list or the request failed.
    Unreachable { reason: String },
}

/// Requests the readiness endpoint on the load balancer's own DNS name with
/// the environment's host header. Only domain-free environments are probed,
/// and a failure is reported, never raised.
pub(super) async fn network_probe(
    url_scheme: &str,
    load_balancer_dns_name: &str,
    hostname: &str,
) -> NetworkProbe {
    if url_scheme != "http" || load_balancer_dns_name.is_empty() {
        return NetworkProbe::Skipped;
    }
    let unreachable = |error: reqwest::Error| NetworkProbe::Unreachable {
        reason: error.without_url().to_string(),
    };
    let client = match reqwest::Client::builder().timeout(PROBE_TIMEOUT).build() {
        Ok(client) => client,
        Err(error) => return unreachable(error),
    };
    match client
        .get(format!("http://{load_balancer_dns_name}{READINESS_PATH}"))
        .header(reqwest::header::HOST, hostname)
        .send()
        .await
    {
        Ok(response) => NetworkProbe::Reachable {
            status: response.status().as_u16(),
        },
        Err(error) => unreachable(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_ecs::types::Deployment;
    use aws_sdk_elasticloadbalancingv2::types::TargetHealth;

    fn service(status: &str, desired: i32, running: i32, state: DeploymentRolloutState) -> Service {
        Service::builder()
            .status(status)
            .desired_count(desired)
            .running_count(running)
            .deployments(
                Deployment::builder()
                    .status("PRIMARY")
                    .rollout_state(state)
                    .build(),
            )
            .build()
    }

    fn target(state: TargetHealthStateEnum) -> TargetHealthDescription {
        TargetHealthDescription::builder()
            .target_health(TargetHealth::builder().state(state).build())
            .build()
    }

    #[test]
    fn a_steady_service_runs_exactly_the_desired_tasks_on_a_completed_deployment() {
        use DeploymentRolloutState::{Completed, InProgress};
        assert!(service_steady(&service("ACTIVE", 2, 2, Completed)));
        assert!(!service_steady(&service("ACTIVE", 2, 1, Completed)));
        assert!(!service_steady(&service("ACTIVE", 2, 3, Completed)));
        assert!(!service_steady(&service("ACTIVE", 0, 0, Completed)));
        assert!(!service_steady(&service("ACTIVE", 1, 1, InProgress)));
        assert!(!service_steady(&service("DRAINING", 1, 1, Completed)));
        assert!(!service_steady(
            &Service::builder().status("ACTIVE").build()
        ));
    }

    #[test]
    fn a_healthy_target_is_any_one_target_reporting_healthy() {
        assert!(any_healthy(&[
            target(TargetHealthStateEnum::Unhealthy),
            target(TargetHealthStateEnum::Healthy)
        ]));
        assert!(!any_healthy(&[
            target(TargetHealthStateEnum::Unhealthy),
            target(TargetHealthStateEnum::Initial)
        ]));
        assert!(!any_healthy(&[]));
    }

    #[test]
    fn readiness_needs_a_steady_service_and_a_healthy_target() {
        assert_eq!(decide(true, true), Readiness::Ready);
        assert_eq!(decide(true, false), Readiness::NoHealthyTarget);
        assert_eq!(decide(false, true), Readiness::ServiceNotSteady);
        assert_eq!(decide(false, false), Readiness::ServiceNotSteady);
    }

    #[tokio::test]
    async fn an_unreachable_probe_is_reported_without_changing_readiness() {
        // Port 1 on loopback refuses connections, so the probe cannot succeed.
        let probe = network_probe("http", "127.0.0.1:1", "pr-12.portal.internal").await;
        assert!(matches!(probe, NetworkProbe::Unreachable { .. }));
        assert_eq!(decide(true, true), Readiness::Ready);
    }

    #[tokio::test]
    async fn a_domain_environment_is_not_probed() {
        assert_eq!(
            network_probe("https", "lb.example.com", "pr-12.example.com").await,
            NetworkProbe::Skipped
        );
        assert_eq!(
            network_probe("http", "", "pr-12.portal.internal").await,
            NetworkProbe::Skipped
        );
    }
}
