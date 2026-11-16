use crate::error::{Error, Result};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

const MAX_CHECKPOINT_BYTES: u64 = 64 * 1024;

/// Persisted operation records are small, hostile JSON, not arbitrary S3 objects.
pub async fn read_checkpoint_body(body: aws_sdk_s3::primitives::ByteStream) -> Result<Vec<u8>> {
    let mut reader = body.into_async_read().take(MAX_CHECKPOINT_BYTES + 1);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(|error| Error::aws_failed("s3", "GetObject", error.to_string()))?;
    if u64::try_from(bytes.len()).map_or(true, |size| size > MAX_CHECKPOINT_BYTES) {
        return Err(Error::state_failed(
            "operation checkpoint exceeds the 65536-byte limit",
        ));
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct OperationId(Uuid);

impl OperationId {
    pub(crate) fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub(crate) fn parse(value: &str) -> Result<Self> {
        let id = Uuid::parse_str(value)
            .map_err(|_| Error::state_failed("operation ID must be a UUIDv4"))?;
        if id.get_version_num() != 4 {
            return Err(Error::state_failed("operation ID must be a UUIDv4"));
        }
        Ok(Self(id))
    }

    pub(crate) fn as_str(&self) -> String {
        self.0.hyphenated().to_string()
    }
}

impl fmt::Display for OperationId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0.hyphenated())
    }
}
impl Serialize for OperationId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for OperationId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct TaskAttemptId(Uuid);

impl TaskAttemptId {
    pub(crate) fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub(crate) fn as_str(&self) -> String {
        self.0.hyphenated().to_string()
    }

    pub(crate) fn new_distinct_from(operation_id: OperationId) -> Self {
        loop {
            let id = Self::new();
            if id.0 != operation_id.0 {
                return id;
            }
        }
    }
}

impl Serialize for TaskAttemptId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for TaskAttemptId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let text = String::deserialize(deserializer)?;
        let id = Uuid::parse_str(&text).map_err(D::Error::custom)?;
        if id.get_version_num() != 4 {
            return Err(D::Error::custom("task attempt ID must be a UUIDv4"));
        }
        Ok(Self(id))
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SecretVersionId(String);

impl SecretVersionId {
    pub(crate) fn parse(value: String) -> Result<Self> {
        if !(32..=64).contains(&value.len()) {
            return Err(Error::state_failed("invalid Secrets Manager version ID"));
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for SecretVersionId {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for SecretVersionId {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaskArn(String);

impl TaskArn {
    pub(crate) fn parse(value: String) -> Result<Self> {
        let mut fields = value.splitn(6, ':');
        let valid_header = fields.next() == Some("arn")
            && fields.next().is_some_and(|partition| !partition.is_empty())
            && fields.next() == Some("ecs")
            && fields.next().is_some_and(|region| !region.is_empty())
            && fields.next().is_some_and(|account| {
                account.len() == 12 && account.bytes().all(|byte| byte.is_ascii_digit())
            });
        let valid_resource = fields
            .next()
            .and_then(|resource| resource.strip_prefix("task/"))
            .is_some_and(|resource| match resource.split_once('/') {
                Some((cluster, task)) => {
                    !cluster.is_empty() && !task.is_empty() && !task.contains('/')
                }
                None => !resource.is_empty(),
            });
        if !valid_header || !valid_resource {
            return Err(Error::state_failed("invalid ECS task ARN"));
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for TaskArn {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for TaskArn {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskAttempt {
    Ready,
    InFlight {
        attempt_id: TaskAttemptId,
        launch: TaskLaunch,
    },
    Failed {
        attempt_id: TaskAttemptId,
        task_arn: TaskArn,
    },
    Succeeded {
        attempt_id: TaskAttemptId,
        task_arn: TaskArn,
    },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "launch", rename_all = "snake_case", deny_unknown_fields)]
pub enum TaskLaunch {
    Reserved,
    Submitted { task_arn: TaskArn },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskTerminal {
    Succeeded,
    Failed,
}

impl TaskAttempt {
    /// A new attempt is legal only on an explicit invocation after an observed
    /// terminal failure. Unknown and in-flight attempts retain their identity.
    pub(crate) fn for_explicit_invocation(&self, candidate: TaskAttemptId) -> Self {
        match self {
            Self::Ready | Self::Failed { .. } => Self::InFlight {
                attempt_id: candidate,
                launch: TaskLaunch::Reserved,
            },
            Self::InFlight { .. } | Self::Succeeded { .. } => self.clone(),
        }
    }

    pub(crate) const fn task_attempt_id(&self) -> Option<TaskAttemptId> {
        match self {
            Self::Ready => None,
            Self::InFlight { attempt_id, .. }
            | Self::Failed { attempt_id, .. }
            | Self::Succeeded { attempt_id, .. } => Some(*attempt_id),
        }
    }

    pub(crate) fn submitted(self, task_arn: TaskArn) -> Result<Self> {
        match self {
            Self::InFlight {
                attempt_id,
                launch: TaskLaunch::Reserved,
            } => Ok(Self::InFlight {
                attempt_id,
                launch: TaskLaunch::Submitted { task_arn },
            }),
            Self::InFlight {
                attempt_id,
                launch: TaskLaunch::Submitted { task_arn: current },
            } if current == task_arn => Ok(Self::InFlight {
                attempt_id,
                launch: TaskLaunch::Submitted { task_arn: current },
            }),
            _ => Err(Error::precondition_failed(
                "task submission does not match its reserved attempt",
            )),
        }
    }

    pub(crate) fn observed_terminal(self, terminal: TaskTerminal) -> Result<Self> {
        match self {
            Self::InFlight {
                attempt_id,
                launch: TaskLaunch::Submitted { task_arn },
            } => match terminal {
                TaskTerminal::Succeeded => Ok(Self::Succeeded {
                    attempt_id,
                    task_arn,
                }),
                TaskTerminal::Failed => Ok(Self::Failed {
                    attempt_id,
                    task_arn,
                }),
            },
            _ => Err(Error::precondition_failed(
                "task completion requires a known running attempt",
            )),
        }
    }

    pub(crate) const fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SeedDumpKey(String);

impl SeedDumpKey {
    pub(crate) fn for_operation(id: OperationId) -> Self {
        Self(format!("operations/seed/{id}.dump"))
    }

    pub(crate) fn parse(value: String, id: OperationId) -> Result<Self> {
        if value != Self::for_operation(id).0 {
            return Err(Error::state_failed(
                "seed dump key does not match its operation ID",
            ));
        }
        Ok(Self(value))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for SeedDumpKey {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for SeedDumpKey {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let resource = value
            .strip_prefix("operations/seed/")
            .and_then(|resource| resource.strip_suffix(".dump"));
        if !resource.is_some_and(|resource| !resource.is_empty() && !resource.contains('/')) {
            return Err(D::Error::custom("invalid seed dump key"));
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Checkpoint<T> {
    pub(crate) record: T,
    pub(crate) object_version_id: String,
    pub(crate) etag: String,
}

impl<T> Checkpoint<T> {
    pub(crate) fn from_s3(
        record: T,
        object_version_id: Option<&str>,
        etag: Option<&str>,
    ) -> Result<Self> {
        let object_version_id = object_version_id
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::state_failed("operation record is missing its S3 version ID"))?;
        let etag = etag
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::state_failed("operation record is missing its S3 ETag"))?;
        Ok(Self {
            record,
            object_version_id: object_version_id.to_owned(),
            etag: etag.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::test_support::TestResult;
    use serde_json::json;

    const TASK: &str = "arn:aws:ecs:us-east-1:123456789012:task/cluster/id";

    fn an_id() -> TaskAttemptId {
        TaskAttemptId::new()
    }

    fn a_task_arn() -> Result<TaskArn> {
        TaskArn::parse(TASK.to_owned())
    }

    #[test]
    fn operation_id_roundtrips_only_uuidv4_values() {
        let id = OperationId::new();
        let encoded = serde_json::to_string(&id).map_err(|error| error.to_string());
        assert_eq!(encoded, Ok(format!("\"{id}\"")));
        let decoded = serde_json::from_str::<OperationId>(&format!("\"{id}\""))
            .map_err(|error| error.to_string());
        assert_eq!(decoded, Ok(id));
        assert!(serde_json::from_str::<OperationId>("\"not-an-id\"").is_err());
        assert!(
            serde_json::from_str::<OperationId>("\"00000000-0000-0000-0000-000000000000\"")
                .is_err()
        );
    }

    #[test]
    fn in_flight_attempt_reuses_token_and_task_arn_until_terminal_observation() -> TestResult {
        let attempt_id = an_id();
        let reserved = TaskAttempt::Ready.for_explicit_invocation(attempt_id);
        assert_eq!(reserved.task_attempt_id(), Some(attempt_id));
        assert_eq!(reserved.for_explicit_invocation(an_id()), reserved);
        let running = reserved.submitted(a_task_arn()?);
        assert_eq!(
            running.as_ref().map(TaskAttempt::task_attempt_id).ok(),
            Some(Some(attempt_id))
        );
        let running = running?;
        assert_eq!(running.for_explicit_invocation(an_id()), running);
        assert!(matches!(
            running,
            TaskAttempt::InFlight {
                launch: TaskLaunch::Submitted { task_arn },
                ..
            } if task_arn.as_str() == TASK
        ));
        Ok(())
    }

    #[test]
    fn only_observed_stopped_failure_allows_a_fresh_explicit_attempt() -> TestResult {
        let first = an_id();
        let failed = TaskAttempt::Ready
            .for_explicit_invocation(first)
            .submitted(a_task_arn()?)
            .and_then(|attempt| attempt.observed_terminal(TaskTerminal::Failed));
        assert_eq!(failed.as_ref().map(TaskAttempt::is_failed).ok(), Some(true));
        let failed = failed?;
        let retried = failed.for_explicit_invocation(an_id());
        assert_ne!(retried.task_attempt_id(), Some(first));
        assert!(matches!(
            retried,
            TaskAttempt::InFlight {
                launch: TaskLaunch::Reserved,
                ..
            }
        ));
        Ok(())
    }

    #[test]
    fn malformed_persisted_attempt_ids_and_unknown_states_are_rejected() {
        let malformed =
            json!({"status":"in_flight", "attempt_id":"invalid", "launch":{"launch":"reserved"}});
        assert!(serde_json::from_value::<TaskAttempt>(malformed).is_err());
        let unknown = json!({"status":"waiting", "attempt_id":an_id().as_str()});
        assert!(serde_json::from_value::<TaskAttempt>(unknown).is_err());
    }

    #[test]
    fn seed_object_key_is_derived_from_persisted_operation_id() {
        let id = OperationId::new();
        let key = SeedDumpKey::for_operation(id);
        assert_eq!(
            SeedDumpKey::parse(key.as_str().to_owned(), id).ok(),
            Some(key)
        );
        assert!(SeedDumpKey::parse("operations/seed/other.dump".to_owned(), id).is_err());
    }

    #[test]
    fn ecs_task_arn_parser_accepts_both_resource_forms_and_rejects_malformed_fields() {
        // Given long and short ECS task ARNs, plus malformed untrusted strings.
        for arn in [
            TASK,
            "arn:aws-us-gov:ecs:us-gov-west-1:123456789012:task/task-id",
        ] {
            // When parsed, then the exact original ARN is retained.
            assert_eq!(
                TaskArn::parse(arn.to_owned()).map(|arn| arn.0).ok(),
                Some(arn.to_owned())
            );
        }
        for arn in [
            "",
            "arn:aws:ecs:us-east-1:123456789012",
            "arn::ecs:us-east-1:123456789012:task/id",
            "arn:aws:s3:us-east-1:123456789012:task/id",
            "arn:aws:ecs::123456789012:task/id",
            "arn:aws:ecs:us-east-1:12345678901x:task/id",
            "arn:aws:ecs:us-east-1:123:task/id",
            "arn:aws:ecs:us-east-1:123456789012:service/id",
            "arn:aws:ecs:us-east-1:123456789012:task/",
            "arn:aws:ecs:us-east-1:123456789012:task//id",
            "arn:aws:ecs:us-east-1:123456789012:task/cluster/",
            "arn:aws:ecs:us-east-1:123456789012:task/cluster/id/extra",
        ] {
            assert_eq!(
                TaskArn::parse(arn.to_owned()).map_err(|error| error.code()),
                Err("STATE_FAILED"),
                "{arn}"
            );
        }
    }

    #[tokio::test]
    async fn checkpoint_reader_accepts_the_exact_budget_and_rejects_one_extra_byte() -> TestResult {
        // Given bodies at and beyond the small persisted-record boundary.
        for size in [0, 1, 65536] {
            let bytes = vec![b' '; size];
            // When read, then every allowed byte is retained.
            assert_eq!(
                read_checkpoint_body(aws_sdk_s3::primitives::ByteStream::from(bytes.clone()))
                    .await?,
                bytes
            );
        }
        for size in [65537, 131_072] {
            let result =
                read_checkpoint_body(aws_sdk_s3::primitives::ByteStream::from(vec![b' '; size]))
                    .await;
            assert_eq!(
                result.map_err(|error| error.to_string()),
                Err("operation checkpoint exceeds the 65536-byte limit".to_owned())
            );
        }
        Ok(())
    }

    #[test]
    fn seed_key_decoder_rejects_empty_and_nested_resources_without_byte_indexing() {
        // Given malformed strings, including multibyte characters near suffix boundaries.
        for key in [
            "operations/seed/.dump",
            "operations/seed/a/b.dump",
            "operations/seed/é.dump/b",
            "operations/seed/a.dumpé",
            "é",
        ] {
            // When deserialized, then malformed persisted keys fail closed.
            assert!(
                serde_json::from_value::<SeedDumpKey>(json!(key)).is_err(),
                "{key}"
            );
        }
    }
}
