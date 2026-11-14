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

    const NOW: i64 = 1_800_000_000;

    fn found(age: Option<i64>, critical: i32, high: i32) -> Found {
        Found {
            completed_at: age.map(|age| NOW - age),
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
}
