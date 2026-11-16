//! Image scan gate: an image is deployable only when its ECR scan completed
//! within the last day and stays within the configured severity limits.
use crate::{
    ctx::Ctx,
    error::{Error, Result},
    terraform::{self, Stack, output_str},
};
use aws_sdk_ecr::{
    client::Waiters,
    types::{FindingSeverity, ImageIdentifier},
};
use serde::Serialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MAX_AGE_SECS: i64 = 86_400;
const CLOCK_SKEW_SECS: i64 = 60;

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ScanResult {
    image_digest: String,
    critical: i32,
    high: i32,
}

pub(crate) async fn scan(ctx: &Ctx, digest: &str) -> Result<ScanResult> {
    super::build::validate_digest(digest)?;
    let base = terraform::required_outputs(ctx, Stack::Base).await?;
    let repository = output_str(&base, "ecr_repository_name")?;
    let image = ImageIdentifier::builder().image_digest(digest).build();
    // The repository scans on push, so a scan is usually running or done and
    // ECR refuses another one; that refusal is the expected case.
    let started = ctx
        .ecr
        .start_image_scan()
        .repository_name(repository)
        .image_id(image.clone())
        .send()
        .await;
    if let Err(error) = started {
        let refused = error.as_service_error().is_some_and(
            aws_sdk_ecr::operation::start_image_scan::StartImageScanError::is_limit_exceeded_exception,
        );
        if !refused {
            return Err(Error::aws_failed(
                "ecr",
                "StartImageScan",
                error.to_string(),
            ));
        }
    }
    let described = ctx
        .ecr
        .wait_until_image_scan_complete()
        .repository_name(repository)
        .image_id(image)
        .wait(Duration::from_mins(15))
        .await
        .map_err(|error| Error::waiter("image scan", digest, error))?
        .into_result()
        .map_err(|error| {
            Error::aws_failed("ecr", "DescribeImageScanFindings", error.to_string())
        })?;
    let findings = described.image_scan_findings();
    let count = |severity| {
        let counts = findings.and_then(|found| found.finding_severity_counts());
        counts
            .and_then(|counts| counts.get(&severity))
            .copied()
            .unwrap_or(0)
    };
    let found = Found {
        completed_at: findings
            .and_then(|found| found.image_scan_completed_at())
            .map(aws_smithy_types::DateTime::secs),
        critical: count(FindingSeverity::Critical),
        high: count(FindingSeverity::High),
    };
    let settings = &ctx.settings.settings;
    let limits = (settings.max_critical_findings, settings.max_high_findings);
    gate(digest, &found, limits, unix_now())
}

struct Found {
    completed_at: Option<i64>,
    critical: i32,
    high: i32,
}

/// The decision itself, separate from the AWS calls.
fn gate(digest: &str, found: &Found, limits: (u32, u32), now: i64) -> Result<ScanResult> {
    let fresh = found.completed_at.is_some_and(|completed| {
        (-CLOCK_SKEW_SECS..=MAX_AGE_SECS).contains(&now.saturating_sub(completed))
    });
    if !fresh {
        return Err(Error::precondition_failed(format!(
            "{digest} has no scan from the last 24 hours"
        )));
    }
    let (max_critical, max_high) = limits;
    if i64::from(found.critical) > i64::from(max_critical)
        || i64::from(found.high) > i64::from(max_high)
    {
        return Err(Error::precondition_failed(format!(
            "{digest} has {} critical and {} high findings; the limits are {max_critical} and {max_high}",
            found.critical, found.high
        )));
    }
    Ok(ScanResult {
        image_digest: digest.to_owned(),
        critical: found.critical,
        high: found.high,
    })
}

fn unix_now() -> i64 {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH);
    elapsed.map_or(0, |elapsed| {
        i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_ecr::{
        operation::{
            describe_image_scan_findings::{
                DescribeImageScanFindingsError, DescribeImageScanFindingsOutput,
            },
            start_image_scan::{StartImageScanError, StartImageScanOutput},
        },
        types::{
            FindingSeverity, ImageScanFindings, ImageScanStatus, ScanStatus,
            error::LimitExceededException,
        },
    };
    use aws_sdk_s3::{operation::get_object::GetObjectOutput, primitives::ByteStream};
    use aws_smithy_mocks::{Rule, RuleMode, mock, mock_client};
    use std::time::SystemTime;

    const DIGEST: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
    const BASE_STATE: &str = r#"{"outputs":{"ecr_repository_name":{"value":"portal-images"}}}"#;

    #[derive(Clone, Copy)]
    enum Start {
        Succeeds,
        LimitRefused,
        Fails,
    }

    async fn run(
        start_behavior: Start,
        waiter_fails: bool,
    ) -> std::result::Result<(Result<ScanResult>, Rule, Rule), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let mut ctx = Ctx::sample(dir.path(), true)?;
        ctx.settings.settings.max_critical_findings = 2;
        ctx.settings.settings.max_high_findings = 3;
        let base = mock!(aws_sdk_s3::Client::get_object)
            .match_requests(|request| request.key() == Some("base/terraform.tfstate"))
            .then_output(|| {
                GetObjectOutput::builder()
                    .body(ByteStream::from_static(BASE_STATE.as_bytes()))
                    .build()
            });
        ctx.s3 = mock_client!(aws_sdk_s3, RuleMode::MatchAny, [&base]);
        let start = match start_behavior {
            Start::Succeeds => mock!(aws_sdk_ecr::Client::start_image_scan)
                .match_requests(|request| {
                    request.repository_name() == Some("portal-images")
                        && request.image_id().and_then(|image| image.image_digest()) == Some(DIGEST)
                })
                .then_output(|| StartImageScanOutput::builder().build()),
            Start::LimitRefused => mock!(aws_sdk_ecr::Client::start_image_scan)
                .match_requests(|request| {
                    request.repository_name() == Some("portal-images")
                        && request.image_id().and_then(|image| image.image_digest()) == Some(DIGEST)
                })
                .then_error(|| {
                    StartImageScanError::LimitExceededException(
                        LimitExceededException::builder()
                            .message("already scanning")
                            .build(),
                    )
                }),
            Start::Fails => mock!(aws_sdk_ecr::Client::start_image_scan)
                .match_requests(|request| {
                    request.repository_name() == Some("portal-images")
                        && request.image_id().and_then(|image| image.image_digest()) == Some(DIGEST)
                })
                .then_error(|| StartImageScanError::unhandled("private start diagnostic")),
        };
        let describe = if waiter_fails {
            mock!(aws_sdk_ecr::Client::describe_image_scan_findings)
                .match_requests(|request| {
                    request.repository_name() == Some("portal-images")
                        && request.image_id().and_then(|image| image.image_digest()) == Some(DIGEST)
                })
                .then_error(|| {
                    DescribeImageScanFindingsError::unhandled("private waiter diagnostic")
                })
        } else {
            mock!(aws_sdk_ecr::Client::describe_image_scan_findings)
                .match_requests(|request| {
                    request.repository_name() == Some("portal-images")
                        && request.image_id().and_then(|image| image.image_digest()) == Some(DIGEST)
                })
                .then_output(|| {
                    let findings = ImageScanFindings::builder()
                        .image_scan_completed_at(
                            aws_smithy_types::DateTime::from(SystemTime::now()),
                        )
                        .finding_severity_counts(FindingSeverity::Critical, 1)
                        .finding_severity_counts(FindingSeverity::High, 2)
                        .build();
                    DescribeImageScanFindingsOutput::builder()
                        .image_scan_status(
                            ImageScanStatus::builder()
                                .status(ScanStatus::Complete)
                                .build(),
                        )
                        .image_scan_findings(findings)
                        .build()
                })
        };
        ctx.ecr = mock_client!(aws_sdk_ecr, RuleMode::MatchAny, [&start, &describe]);
        let result = scan(&ctx, DIGEST).await;
        Ok((result, start, describe))
    }

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    #[tokio::test]
    async fn scan_uses_the_requested_digest_and_maps_actual_severity_counts() -> TestResult {
        let (result, start, describe) = run(Start::Succeeds, false).await?;

        assert_eq!(
            result?,
            ScanResult {
                image_digest: DIGEST.to_owned(),
                critical: 1,
                high: 2,
            }
        );
        assert_eq!((start.num_calls(), describe.num_calls()), (1, 1));
        Ok(())
    }

    #[tokio::test]
    async fn an_ecr_scan_limit_refusal_uses_the_completed_existing_scan() -> TestResult {
        let (result, start, describe) = run(Start::LimitRefused, false).await?;

        assert_eq!(
            result?,
            ScanResult {
                image_digest: DIGEST.to_owned(),
                critical: 1,
                high: 2,
            }
        );
        assert_eq!((start.num_calls(), describe.num_calls()), (1, 1));
        Ok(())
    }

    #[tokio::test]
    async fn an_unexpected_scan_start_failure_stops_before_waiting() -> TestResult {
        let (result, start, describe) = run(Start::Fails, false).await?;

        assert_eq!(
            result.map(|_| ()).map_err(|error| error.code()),
            Err("AWS_FAILED")
        );
        assert_eq!((start.num_calls(), describe.num_calls()), (1, 0));
        Ok(())
    }

    #[tokio::test]
    async fn an_ecr_waiter_failure_is_returned_after_starting_the_scan() -> TestResult {
        let (result, start, describe) = run(Start::Succeeds, true).await?;

        assert_eq!(
            result.map(|_| ()).map_err(|error| error.code()),
            Err("AWS_FAILED")
        );
        assert_eq!((start.num_calls(), describe.num_calls()), (1, 1));
        Ok(())
    }

    const NOW: i64 = 1_800_000_000;

    fn found(age: Option<i64>, critical: i32, high: i32) -> Found {
        Found {
            completed_at: age.and_then(|age| NOW.checked_sub(age)),
            critical,
            high,
        }
    }

    #[test]
    fn a_recent_scan_within_the_limits_passes() {
        let result = gate("sha256:aa", &found(Some(30), 0, 2), (0, 2), NOW);
        let expected = ScanResult {
            image_digest: "sha256:aa".to_owned(),
            critical: 0,
            high: 2,
        };
        assert_eq!(result.ok(), Some(expected));
    }

    #[test]
    fn findings_over_either_limit_block_the_image() {
        for (critical, high) in [(1, 0), (0, 3)] {
            let result = gate("sha256:aa", &found(Some(30), critical, high), (0, 2), NOW);
            assert_eq!(
                result.map_err(|error| error.code()),
                Err("PRECONDITION_FAILED")
            );
        }
    }

    #[test]
    fn a_missing_stale_or_future_scan_blocks_the_image() {
        for age in [None, Some(MAX_AGE_SECS + 1), Some(-CLOCK_SKEW_SECS - 1)] {
            let result = gate("sha256:aa", &found(age, 0, 0), (0, 0), NOW);
            assert_eq!(
                result.map_err(|error| error.code()),
                Err("PRECONDITION_FAILED")
            );
        }
    }

    #[test]
    fn scans_at_both_age_boundaries_are_accepted_and_adjacent_ages_are_rejected() {
        for (age, expected) in [
            (MAX_AGE_SECS, Ok((0, 0))),
            (-CLOCK_SKEW_SECS, Ok((0, 0))),
            (MAX_AGE_SECS + 1, Err("PRECONDITION_FAILED")),
            (-CLOCK_SKEW_SECS - 1, Err("PRECONDITION_FAILED")),
        ] {
            let result = gate("sha256:aa", &found(Some(age), 0, 0), (0, 0), NOW)
                .map(|scan| (scan.critical, scan.high))
                .map_err(|error| error.code());
            assert_eq!(result, expected, "age={age}");
        }
    }
}
