use crate::{
    error::{Error, Result},
    settings::ValidatedSettings,
};
use serde::Serialize;
use serde_json::Value;
use std::{fs::OpenOptions, io::Write, path::PathBuf, time::Instant};

/// Everything a command needs: validated settings, AWS clients, Terraform.
pub(crate) struct Ctx {
    pub(crate) settings: ValidatedSettings,
    pub(crate) project_dir: PathBuf,
    pub(crate) approve: bool,
    pub(crate) sts: aws_sdk_sts::Client,
    pub(crate) ecr: aws_sdk_ecr::Client,
    pub(crate) ecs: aws_sdk_ecs::Client,
    pub(crate) elb: aws_sdk_elasticloadbalancingv2::Client,
    pub(crate) s3: aws_sdk_s3::Client,
    pub(crate) secrets: aws_sdk_secretsmanager::Client,
    pub(crate) terraform: PathBuf,
}
impl Ctx {
    pub(crate) fn new(settings: ValidatedSettings, aws: &aws_config::SdkConfig) -> Self {
        Self {
            project_dir: settings.settings.project.clone(),
            approve: settings.settings.approve,
            sts: aws_sdk_sts::Client::new(aws),
            ecr: aws_sdk_ecr::Client::new(aws),
            ecs: aws_sdk_ecs::Client::new(aws),
            elb: aws_sdk_elasticloadbalancingv2::Client::new(aws),
            s3: aws_sdk_s3::Client::new(aws),
            secrets: aws_sdk_secretsmanager::Client::new(aws),
            terraform: settings.settings.terraform_path(),
            settings,
        }
    }

    /// A context with no credentials whose project directory is `project`.
    /// Tests replace the clients they exercise with mocks.
    #[cfg(test)]
    pub(crate) fn sample(project: &std::path::Path, approve: bool) -> Result<Self> {
        let mut settings = crate::settings::Settings::sample(project.to_path_buf());
        settings.approve = approve;
        let aws = aws_config::SdkConfig::builder()
            .behavior_version(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new("us-east-1"))
            .build();
        Ok(Self::new(settings.validate()?, &aws))
    }

    /// Every command that changes anything calls this before doing so.
    pub(crate) fn require_approval(&self) -> Result<()> {
        if self.approve {
            return Ok(());
        }
        Err(Error::approval_required(
            "this command changes AWS or GitHub; rerun it with --approve",
        ))
    }
}
#[derive(Clone, Copy)]
pub(crate) enum EventStatus {
    Started,
    Ok,
    Failed,
}
impl EventStatus {
    const fn value(self) -> &'static str {
        match self {
            Self::Started => "started",
            Self::Ok => "ok",
            Self::Failed => "failed",
        }
    }
}
pub(crate) fn emit_event(name: &str, status: EventStatus, started: Instant) -> Result<()> {
    let line=serde_json::to_vec(&serde_json::json!({"event":"step","name":name,"status":status.value(),"elapsed_ms":started.elapsed().as_millis()})).map_err(|error|Error::state_failed(error.to_string()))?;
    let mut stderr = std::io::stderr().lock();
    stderr
        .write_all(&line)
        .and_then(|()| stderr.write_all(b"\n"))
        .map_err(|error| Error::state_failed(error.to_string()))
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct StepTiming {
    pub(crate) name: String,
    pub(crate) status: String,
    pub(crate) elapsed_ms: u128,
}
/// Calls `probe` every `interval` until it yields a value, for at most
/// `attempts` calls. SDK waiters back off exponentially, which is too slow for
/// a deployment that should finish within minutes.
pub(crate) async fn poll<T, F>(
    what: &'static str,
    remote_id: &str,
    interval: std::time::Duration,
    attempts: u32,
    mut probe: impl FnMut() -> F,
) -> Result<T>
where
    F: Future<Output = Result<Option<T>>>,
{
    for _ in 0..attempts {
        if let Some(value) = probe().await? {
            return Ok(value);
        }
        tokio::time::sleep(interval).await;
    }
    Err(Error::remote_timeout(what, remote_id))
}
/// Runs one named step: start and finish events on stderr, plus its timing.
pub(crate) async fn step<T>(
    name: &str,
    work: impl Future<Output = Result<T>>,
) -> Result<(T, StepTiming)> {
    let started = Instant::now();
    emit_event(name, EventStatus::Started, started)?;
    let outcome = work.await;
    let status = if outcome.is_ok() {
        EventStatus::Ok
    } else {
        EventStatus::Failed
    };
    emit_event(name, status, started)?;
    let timing = StepTiming {
        name: name.to_owned(),
        status: status.value().to_owned(),
        elapsed_ms: started.elapsed().as_millis(),
    };
    Ok((outcome?, timing))
}
/// Appends the step timeline to the GitHub job summary when one is available.
pub(crate) fn write_summary(timings: &[StepTiming]) -> Result<()> {
    let Some(path) = std::env::var_os("GITHUB_STEP_SUMMARY") else {
        return Ok(());
    };
    let rows = timings.iter().map(|timing| {
        let StepTiming {
            name,
            status,
            elapsed_ms,
        } = timing;
        format!("| {name} | {status} | {elapsed_ms} |\n")
    });
    let header = "## backstage-aws timeline\n\n| Step | Status | Elapsed (ms) |\n|---|---|---:|\n";
    let table = format!("{header}{}", rows.collect::<String>());
    OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| file.write_all(table.as_bytes()))
        .map_err(|error| Error::state_failed(error.to_string()))
}
pub(crate) fn print_success(command: &str, result: impl Serialize) -> Result<()> {
    print_envelope(&serde_json::json!({"ok":true,"command":command,"result":result}))
}
pub(crate) fn print_failure(command: &str, error: &Error) -> Result<()> {
    let mut failure = serde_json::json!({
        "code": error.code(),
        "message": error.to_string(),
        "retryable": error.retryable()
    });
    if let Error::SetupBlocked(report) = error {
        let details =
            serde_json::to_value(report).map_err(|error| Error::state_failed(error.to_string()))?;
        failure
            .as_object_mut()
            .ok_or_else(|| Error::state_failed("error envelope is not an object"))?
            .insert(String::from("details"), details);
    }
    print_envelope(&serde_json::json!({"ok":false,"command":command,"error":failure}))
}
fn print_envelope(value: &Value) -> Result<()> {
    let bytes =
        serde_json::to_vec(value).map_err(|error| Error::state_failed(error.to_string()))?;
    let mut out = std::io::stdout().lock();
    out.write_all(&bytes)
        .and_then(|()| out.write_all(b"\n"))
        .map_err(|error| Error::state_failed(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, time::Duration};

    const INTERVAL: Duration = Duration::from_secs(3);

    async fn probe_until(ready_on: u32, attempts: u32) -> (Result<u32>, u32, Duration) {
        let calls = Cell::new(0_u32);
        let started = tokio::time::Instant::now();
        let outcome = poll("deployment", "service-1", INTERVAL, attempts, || async {
            calls.set(calls.get().saturating_add(1));
            Ok((calls.get() == ready_on).then_some(calls.get()))
        })
        .await;
        (outcome, calls.get(), started.elapsed())
    }

    #[tokio::test(start_paused = true)]
    async fn polling_returns_as_soon_as_the_probe_yields_a_value() {
        let (outcome, calls, waited) = probe_until(3, 10).await;
        assert_eq!(outcome.ok(), Some(3));
        assert_eq!(calls, 3);
        assert_eq!(waited, Duration::from_secs(6));
    }

    #[tokio::test(start_paused = true)]
    async fn polling_gives_up_as_a_retryable_timeout_naming_the_remote_work() {
        let (outcome, calls, _) = probe_until(99, 4).await;
        let error = outcome.err();
        assert_eq!(calls, 4);
        assert_eq!(error.as_ref().map(Error::exit_code), Some(5));
        assert_eq!(
            error.map(|error| error.to_string()),
            Some("deployment continues remotely (service-1)".to_owned())
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_failing_probe_stops_polling_at_once() {
        let calls = Cell::new(0_u32);
        let outcome: Result<()> = poll("deployment", "service-1", INTERVAL, 10, || async {
            calls.set(calls.get().saturating_add(1));
            Err(Error::aws_failed("ecs", "DescribeServices", "denied"))
        })
        .await;
        assert_eq!(outcome.map_err(|error| error.code()), Err("AWS_FAILED"));
        assert_eq!(calls.get(), 1);
    }

    #[tokio::test]
    async fn a_step_reports_its_timing_and_passes_the_failure_through() -> Result<()> {
        let (value, timing) = step("image.build", async { Ok(7) }).await?;
        assert_eq!(
            (value, timing.name.as_str(), timing.status.as_str()),
            (7, "image.build", "ok")
        );
        let failed = step("image.build", async {
            Err::<(), _>(Error::tool_failed("docker", "exit 1"))
        });
        assert_eq!(
            failed.await.map(drop).map_err(|error| error.code()),
            Err("TOOL_FAILED")
        );
        Ok(())
    }
}
