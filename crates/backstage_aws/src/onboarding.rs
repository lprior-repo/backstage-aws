//! Read-only prerequisite discovery.
pub mod model;
mod probe;

use crate::{
    error::{Error, Result},
    settings::Settings,
};
use clap::Args;
use model::ReportStatus;
use std::path::PathBuf;

#[derive(Args, Clone, Debug)]
pub(crate) struct SetupArgs {
    /// AWS CLI v2 executable used for read-only account discovery.
    #[arg(long, default_value = "aws")]
    pub(crate) aws_cli: PathBuf,
    /// Short-lived AWS CLI profile; never supply access keys.
    #[arg(long, env = "AWS_PROFILE")]
    pub(crate) profile: Option<String>,
}

pub(crate) async fn run(settings: &Settings, args: &SetupArgs) -> Result<serde_json::Value> {
    settings.validate_network()?;
    let checks = probe::collect(settings, args).await?;
    let report = model::report(settings, args, checks)?;
    if report.status == ReportStatus::Blocked {
        return Err(Error::SetupBlocked(Box::new(report)));
    }
    serde_json::to_value(report).map_err(|error| Error::state_failed(error.to_string()))
}
