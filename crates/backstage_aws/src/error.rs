use thiserror::Error as ThisError;
#[derive(Debug, ThisError)]
pub(crate) enum Error {
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    ApprovalRequired(String),
    #[error("{0}")]
    PreconditionFailed(String),
    #[error("{tool}: {detail}")]
    ToolFailed { tool: String, detail: String },
    #[error("{service} {operation}: {detail}")]
    AwsFailed {
        service: &'static str,
        operation: &'static str,
        detail: String,
    },
    #[error("{0}")]
    GithubFailed(String),
    #[error("{0}")]
    StateFailed(String),
    #[error("{0}")]
    DbTaskFailed(String),
    #[error("{operation} continues remotely ({remote_id})")]
    RemoteTimeout {
        operation: &'static str,
        remote_id: String,
    },
}
pub(crate) type Result<T> = std::result::Result<T, Error>;
impl Error {
    pub(crate) fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }
    pub(crate) fn approval_required(message: impl Into<String>) -> Self {
        Self::ApprovalRequired(message.into())
    }
    pub(crate) fn precondition_failed(message: impl Into<String>) -> Self {
        Self::PreconditionFailed(message.into())
    }
    pub(crate) fn tool_failed(tool: impl Into<String>, detail: impl Into<String>) -> Self {
        Self::ToolFailed {
            tool: tool.into(),
            detail: detail.into(),
        }
    }
    pub(crate) fn aws_failed(
        service: &'static str,
        operation: &'static str,
        detail: impl Into<String>,
    ) -> Self {
        Self::AwsFailed {
            service,
            operation,
            detail: detail.into(),
        }
    }
    pub(crate) fn github_failed(message: impl Into<String>) -> Self {
        Self::GithubFailed(message.into())
    }
    pub(crate) fn state_failed(message: impl Into<String>) -> Self {
        Self::StateFailed(message.into())
    }
    pub(crate) fn remote_timeout(operation: &'static str, remote_id: impl Into<String>) -> Self {
        Self::RemoteTimeout {
            operation,
            remote_id: remote_id.into(),
        }
    }
    pub(crate) fn db_task_failed(message: impl Into<String>) -> Self {
        Self::DbTaskFailed(message.into())
    }
    /// A failed SDK waiter: running out of time means the work continues in
    /// AWS, anything else is a failure.
    pub(crate) fn waiter<O, E>(
        operation: &'static str,
        remote_id: &str,
        error: aws_smithy_runtime_api::client::waiters::error::WaiterError<O, E>,
    ) -> Self
    where
        aws_smithy_runtime_api::client::waiters::error::WaiterError<O, E>: std::fmt::Display,
    {
        use aws_smithy_runtime_api::client::waiters::error::WaiterError;
        match error {
            WaiterError::ExceededMaxWait(_) => Self::remote_timeout(operation, remote_id),
            other => Self::aws_failed("waiter", operation, other.to_string()),
        }
    }
    pub(crate) const fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "INVALID_INPUT",
            Self::ApprovalRequired(_) => "APPROVAL_REQUIRED",
            Self::PreconditionFailed(_) => "PRECONDITION_FAILED",
            Self::ToolFailed { .. } => "TOOL_FAILED",
            Self::AwsFailed { .. } => "AWS_FAILED",
            Self::GithubFailed(_) => "GITHUB_FAILED",
            Self::StateFailed(_) => "STATE_FAILED",
            Self::RemoteTimeout { .. } => "REMOTE_TIMEOUT",
            Self::DbTaskFailed(_) => "DB_TASK_FAILED",
        }
    }
    pub(crate) const fn exit_code(&self) -> u8 {
        match self {
            Self::InvalidInput(_) => 2,
            Self::ApprovalRequired(_) | Self::PreconditionFailed(_) => 3,
            Self::RemoteTimeout { .. } => 5,
            _ => 4,
        }
    }
    pub(crate) const fn retryable(&self) -> bool {
        matches!(self, Self::RemoteTimeout { .. })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_codes_exit_status_and_retryability_match_contract() {
        let cases = [
            (Error::invalid_input("x"), "INVALID_INPUT", 2, false),
            (Error::approval_required("x"), "APPROVAL_REQUIRED", 3, false),
            (
                Error::precondition_failed("x"),
                "PRECONDITION_FAILED",
                3,
                false,
            ),
            (Error::tool_failed("tool", "x"), "TOOL_FAILED", 4, false),
            (
                Error::aws_failed("ecs", "DescribeServices", "x"),
                "AWS_FAILED",
                4,
                false,
            ),
            (Error::github_failed("x"), "GITHUB_FAILED", 4, false),
            (Error::state_failed("x"), "STATE_FAILED", 4, false),
            (
                Error::db_task_failed("task exited nonzero"),
                "DB_TASK_FAILED",
                4,
                false,
            ),
            (
                Error::remote_timeout("scan", "scan-id"),
                "REMOTE_TIMEOUT",
                5,
                true,
            ),
        ];
        for (error, code, exit, retryable) in cases {
            assert_eq!(error.code(), code);
            assert_eq!(error.exit_code(), exit);
            assert_eq!(error.retryable(), retryable);
        }
    }
}
