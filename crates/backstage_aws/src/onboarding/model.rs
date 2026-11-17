mod actions;
use actions::{action, doctor_action, resume_command};

use serde::Serialize;
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stage {
    AwsCli,
    Credentials,
    Account,
    Organization,
    DeploymentSettings,
    Project,
    LocalTools,
}
impl Stage {
    pub(crate) const ORDER: [Self; 7] = [
        Self::AwsCli,
        Self::Credentials,
        Self::Account,
        Self::Organization,
        Self::DeploymentSettings,
        Self::Project,
        Self::LocalTools,
    ];
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Verified,
    ActionRequired,
    Blocked,
    NotChecked,
}
#[derive(Debug, Serialize)]
pub(crate) struct Check {
    pub(crate) stage: Stage,
    pub(crate) status: Status,
    pub(crate) detail: String,
}
#[derive(Debug, Serialize)]
pub(crate) struct Action {
    pub(crate) title: String,
    pub(crate) instructions: Vec<String>,
    pub(crate) command: Option<Vec<String>>,
    pub(crate) url: Option<String>,
}
#[derive(Debug, Serialize)]
pub(crate) struct Report {
    pub(crate) status: ReportStatus,
    pub(crate) checks: Vec<Check>,
    pub(crate) next: Option<Action>,
    pub(crate) resume: Vec<String>,
    pub(crate) environment: BTreeMap<String, String>,
}
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReportStatus {
    Blocked,
    ReadyForDoctor,
}
#[cfg(test)]
mod tests;

pub(crate) fn report(
    settings: &crate::settings::Settings,
    args: &super::SetupArgs,
    checks: Vec<Check>,
) -> crate::error::Result<Report> {
    let ordered = ordered_checks(checks)?;
    let next_check = ordered.iter().find(|check| !is_complete(check));
    let ready = ordered.len() == Stage::ORDER.len() && next_check.is_none();
    let next = match next_check {
        Some(check) => Some(action(settings, args, check)),
        None => Some(doctor_action(settings)?),
    };
    let resume = resume_command(settings, args)?;
    let environment = args.profile.as_ref().map_or_else(BTreeMap::new, |profile| {
        BTreeMap::from([(String::from("AWS_PROFILE"), profile.clone())])
    });
    Ok(Report {
        status: if ready {
            ReportStatus::ReadyForDoctor
        } else {
            ReportStatus::Blocked
        },
        checks: ordered,
        next,
        resume,
        environment,
    })
}

fn ordered_checks(mut checks: Vec<Check>) -> crate::error::Result<Vec<Check>> {
    checks.sort_unstable_by_key(|check| check.stage);
    if checks.windows(2).any(|pair| {
        pair.first()
            .zip(pair.last())
            .is_some_and(|(first, last)| first.stage == last.stage)
    }) {
        return Err(crate::error::Error::invalid_input(
            "duplicate onboarding check stage",
        ));
    }
    let mut observed = checks.into_iter().peekable();
    Ok(Stage::ORDER
        .into_iter()
        .map(|stage| {
            if let Some(check) = observed.next_if(|check| check.stage == stage) {
                return check;
            }
            Check {
                stage,
                status: Status::NotChecked,
                detail: String::from("not checked"),
            }
        })
        .collect())
}
const fn is_complete(check: &Check) -> bool {
    matches!(check.status, Status::Verified)
}
