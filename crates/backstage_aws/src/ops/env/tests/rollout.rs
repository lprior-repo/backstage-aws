use super::{super::rollout, NEW, OLD, Rollout, service};
use aws_sdk_ecs::types::DeploymentRolloutState::{Completed, Failed, InProgress};

fn outcome(deployments: &[Rollout]) -> Result<bool, &'static str> {
    let done = rollout(&service(deployments), NEW).map(|done| done.is_some());
    done.map_err(|error| error.code())
}

#[test]
fn a_rollout_is_done_only_when_the_new_task_definition_completes() {
    assert_eq!(outcome(&[]), Ok(false));
    assert_eq!(
        outcome(&[("PRIMARY", NEW, InProgress), ("ACTIVE", OLD, Completed)]),
        Ok(false)
    );
    assert_eq!(outcome(&[("PRIMARY", NEW, Completed)]), Ok(true));
    assert_eq!(outcome(&[("PRIMARY", NEW, Failed)]), Err("AWS_FAILED"));
}

#[test]
fn a_circuit_breaker_rollback_is_reported_as_a_failure_not_a_success() {
    assert_eq!(
        outcome(&[("PRIMARY", OLD, InProgress), ("ACTIVE", NEW, Failed)]),
        Err("AWS_FAILED")
    );
    assert_eq!(outcome(&[("PRIMARY", OLD, Completed)]), Err("AWS_FAILED"));
}
