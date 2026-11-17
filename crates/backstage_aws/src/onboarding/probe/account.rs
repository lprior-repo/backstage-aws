use super::aws::{aws_json, credential_or_access};
use super::{Check, Stage, action_required, blocked, verified};
use crate::{
    error::{Error, Result},
    settings::Settings,
};
use std::path::Path;

pub(super) async fn organization(
    program: &Path,
    profile: Option<&str>,
    settings: &Settings,
) -> Result<std::result::Result<Check, Check>> {
    let query = "{Organization:Organization.{Id:Id,MasterAccountId:MasterAccountId}}";
    let result = aws_json(
        program,
        profile,
        settings,
        "organizations",
        "describe-organization",
        &["--query", query],
    )
    .await;
    let value = match result {
        Ok(value) => value,
        Err(Error::AwsFailed { detail, .. })
            if detail.contains("AWSOrganizationsNotInUseException") =>
        {
            return Ok(Err(action_required(
                Stage::Organization,
                "AWS Organizations is not enabled for this account; enable it from the AWS console.",
            )));
        }
        Err(error) => {
            return credential_or_access(
                error,
                Stage::Organization,
                "organizations describe-organization",
            );
        }
    };
    let outer = super::parse::object(
        &value,
        &["Organization"],
        "Organizations DescribeOrganization",
    )?;
    let org = super::parse::object(
        super::parse::value(outer, "Organization", "Organizations DescribeOrganization")?,
        &["Id", "MasterAccountId"],
        "Organizations organization",
    )?;
    let id = super::parse::string(org, "Id", "Organizations organization")?;
    let master = super::parse::string(org, "MasterAccountId", "Organizations organization")?;
    if id.is_empty() || master != settings.aws_account_id {
        return Ok(Err(blocked(
            Stage::Organization,
            "The active account is not the management account of the organization.",
        )));
    }
    Ok(Ok(verified(
        Stage::Organization,
        "AWS Organizations is active and this account is its management account.",
    )))
}
