use crate::{
    db::{OperationId, SecretVersionId, TaskArn, TaskAttempt, TaskAttemptId},
    error::{Error, Result},
};
use serde::{Deserialize, Serialize};

const ROTATION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RotationOperation {
    schema_version: u32,
    pub(super) id: OperationId,
    pub(super) state: RotationState,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum RotationState {
    Prepared,
    BaselinePinned {
        previous_version_id: SecretVersionId,
    },
    SecretStaged {
        previous_version_id: SecretVersionId,
    },
    RoleChange {
        previous_version_id: SecretVersionId,
        attempt: TaskAttempt,
    },
    SecretPromoted {
        previous_version_id: SecretVersionId,
        task_arn: TaskArn,
        promoted_at_epoch_ms: u64,
    },
    ServicesRestarted {
        previous_version_id: SecretVersionId,
        task_arn: TaskArn,
        services_restarted: Vec<String>,
    },
    Completed {
        previous_version_id: SecretVersionId,
        task_arn: TaskArn,
        services_restarted: Vec<String>,
    },
}

impl RotationOperation {
    pub(super) fn new() -> Self {
        Self {
            schema_version: ROTATION_SCHEMA_VERSION,
            id: OperationId::new(),
            state: RotationState::Prepared,
        }
    }

    pub(super) fn validate(self) -> Result<Self> {
        if self.schema_version != ROTATION_SCHEMA_VERSION {
            return Err(Error::state_failed("unsupported rotation operation schema"));
        }
        let replacement = SecretVersionId::parse(self.id.as_str())?;
        if let Some(previous) = self.state.previous_version_id()
            && *previous == replacement
        {
            return Err(Error::state_failed(
                "rotation baseline cannot equal the replacement secret version",
            ));
        }
        if let Some(attempt_id) = self.state.task_attempt_id()
            && attempt_id.as_str() == self.id.as_str()
        {
            return Err(Error::state_failed(
                "rotation task attempt ID cannot equal its operation ID",
            ));
        }
        if matches!(
            &self.state,
            RotationState::SecretPromoted {
                promoted_at_epoch_ms: 0,
                ..
            }
        ) {
            return Err(Error::state_failed(
                "rotation promotion timestamp must be nonzero",
            ));
        }
        Ok(self)
    }

    pub(super) fn replacement_version_id(&self) -> Result<SecretVersionId> {
        SecretVersionId::parse(self.id.as_str())
    }
}

impl RotationState {
    pub(super) const fn previous_version_id(&self) -> Option<&SecretVersionId> {
        match self {
            Self::Prepared => None,
            Self::BaselinePinned {
                previous_version_id,
            }
            | Self::SecretStaged {
                previous_version_id,
            }
            | Self::RoleChange {
                previous_version_id,
                ..
            }
            | Self::SecretPromoted {
                previous_version_id,
                ..
            }
            | Self::ServicesRestarted {
                previous_version_id,
                ..
            }
            | Self::Completed {
                previous_version_id,
                ..
            } => Some(previous_version_id),
        }
    }

    const fn task_attempt_id(&self) -> Option<TaskAttemptId> {
        match self {
            Self::RoleChange { attempt, .. } => attempt.task_attempt_id(),
            _ => None,
        }
    }

    pub(super) const fn is_completed(&self) -> bool {
        matches!(self, Self::Completed { .. })
    }

    pub(super) fn pin_baseline(self, version_id: SecretVersionId) -> Result<Self> {
        match self {
            Self::Prepared => Ok(Self::BaselinePinned {
                previous_version_id: version_id,
            }),
            _ => Err(Self::invalid_transition("BaselinePinned")),
        }
    }

    pub(super) fn stage_secret(self) -> Result<Self> {
        match self {
            Self::BaselinePinned {
                previous_version_id,
            } => Ok(Self::SecretStaged {
                previous_version_id,
            }),
            _ => Err(Self::invalid_transition("SecretStaged")),
        }
    }

    pub(super) fn begin_role_change(self) -> Result<Self> {
        match self {
            Self::SecretStaged {
                previous_version_id,
            } => Ok(Self::RoleChange {
                previous_version_id,
                attempt: TaskAttempt::Ready,
            }),
            _ => Err(Self::invalid_transition("RoleChange")),
        }
    }

    pub(super) fn with_task_attempt(
        self,
        next: TaskAttempt,
        operation_id: OperationId,
    ) -> Result<Self> {
        match self {
            Self::RoleChange {
                previous_version_id,
                attempt,
            } => {
                validate_attempt_transition(&attempt, &next, operation_id)?;
                Ok(Self::RoleChange {
                    previous_version_id,
                    attempt: next,
                })
            }
            _ => Err(Self::invalid_transition("RoleChange task attempt")),
        }
    }

    pub(super) fn promote_secret(self, promoted_at_epoch_ms: u64) -> Result<Self> {
        match self {
            Self::RoleChange {
                previous_version_id,
                attempt: TaskAttempt::Succeeded { task_arn, .. },
            } => Ok(Self::SecretPromoted {
                previous_version_id,
                task_arn,
                promoted_at_epoch_ms,
            }),
            _ => Err(Self::invalid_transition(
                "SecretPromoted requires a successful role-change task",
            )),
        }
    }

    pub(super) fn restart_services(self, services_restarted: Vec<String>) -> Result<Self> {
        match self {
            Self::SecretPromoted {
                previous_version_id,
                task_arn,
                ..
            } if !services_restarted.is_empty()
                && services_restarted.iter().all(|service| !service.is_empty()) =>
            {
                Ok(Self::ServicesRestarted {
                    previous_version_id,
                    task_arn,
                    services_restarted,
                })
            }
            _ => Err(Self::invalid_transition("ServicesRestarted")),
        }
    }

    pub(super) fn complete(self) -> Result<Self> {
        match self {
            Self::ServicesRestarted {
                previous_version_id,
                task_arn,
                services_restarted,
            } => Ok(Self::Completed {
                previous_version_id,
                task_arn,
                services_restarted,
            }),
            _ => Err(Self::invalid_transition("Completed")),
        }
    }

    fn invalid_transition(next: &str) -> Error {
        Error::precondition_failed(format!("invalid rotation transition to {next}"))
    }
}

fn validate_attempt_transition(
    previous: &TaskAttempt,
    next: &TaskAttempt,
    operation_id: OperationId,
) -> Result<()> {
    let operation_id = operation_id.as_str();
    let valid = match (previous, next) {
        (
            TaskAttempt::Ready,
            TaskAttempt::InFlight {
                attempt_id,
                launch: crate::db::TaskLaunch::Reserved,
            },
        ) => attempt_id.as_str() != operation_id,
        (
            TaskAttempt::InFlight {
                attempt_id: previous_id,
                launch: crate::db::TaskLaunch::Reserved,
            },
            TaskAttempt::InFlight {
                attempt_id: next_id,
                launch: crate::db::TaskLaunch::Submitted { .. },
            },
        ) => previous_id == next_id,
        (
            TaskAttempt::InFlight {
                attempt_id: previous_id,
                launch:
                    crate::db::TaskLaunch::Submitted {
                        task_arn: previous_arn,
                    },
            },
            TaskAttempt::Succeeded {
                attempt_id: next_id,
                task_arn: next_arn,
            }
            | TaskAttempt::Failed {
                attempt_id: next_id,
                task_arn: next_arn,
            },
        ) => previous_id == next_id && previous_arn == next_arn,
        (
            TaskAttempt::Failed {
                attempt_id: previous_id,
                ..
            },
            TaskAttempt::InFlight {
                attempt_id,
                launch: crate::db::TaskLaunch::Reserved,
            },
        ) => *previous_id != *attempt_id && attempt_id.as_str() != operation_id,
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(Error::precondition_failed(
            "rotation task attempt transition is not monotonic",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    const TASK: &str = "arn:aws:ecs:us-east-1:123456789012:task/cluster/id";

    fn a_task() -> Result<TaskArn> {
        TaskArn::parse(TASK.to_owned())
    }

    #[test]
    fn transitions_require_adjacent_successful_phases_and_same_attempt() -> TestResult {
        let operation_id = OperationId::new();
        let prepared = RotationState::Prepared;
        assert!(prepared.clone().stage_secret().is_err());
        let baseline = SecretVersionId::parse("old-version-12345678901234567890".to_owned())?;
        let staged = prepared.pin_baseline(baseline)?.stage_secret()?;
        let role_change = staged.begin_role_change()?;
        assert!(role_change.clone().promote_secret(1).is_err());

        let reserved = TaskAttempt::Ready
            .for_explicit_invocation(TaskAttemptId::new_distinct_from(operation_id));
        let role_change = role_change.with_task_attempt(reserved.clone(), operation_id)?;
        let submitted = reserved.submitted(a_task()?)?;
        let role_change = role_change.with_task_attempt(submitted.clone(), operation_id)?;
        let succeeded = submitted
            .clone()
            .observed_terminal(crate::db::TaskTerminal::Succeeded)?;
        let role_change = role_change.with_task_attempt(succeeded, operation_id)?;
        let failed = submitted.observed_terminal(crate::db::TaskTerminal::Failed)?;
        assert!(
            role_change
                .clone()
                .with_task_attempt(failed, operation_id)
                .is_err()
        );
        let complete = role_change
            .promote_secret(1)?
            .restart_services(vec!["service-a".to_owned()])?
            .complete()?;
        assert!(complete.is_completed());
        Ok(())
    }

    #[test]
    fn malformed_operation_schema_and_identity_are_rejected() -> TestResult {
        let malformed = json!({
            "schema_version": 2,
            "id": "11111111-1111-4111-8111-111111111111",
            "state": {"phase": "prepared"}
        });
        let malformed: RotationOperation = serde_json::from_value(malformed)?;
        assert!(malformed.validate().is_err());
        let valid_id = OperationId::new();
        let same_id_baseline = json!({
            "schema_version": 1,
            "id": valid_id,
            "state": {"phase": "baseline_pinned", "previous_version_id": valid_id.as_str()}
        });
        let same_id_baseline: RotationOperation = serde_json::from_value(same_id_baseline)?;
        assert!(same_id_baseline.validate().is_err());
        Ok(())
    }
}
