use super::aws::{aws_json, credential_or_access};
use super::{Check, Stage, action_required, blocked, verified};
use crate::{
    error::{Error, Result},
    settings::Settings,
};
use std::path::Path;

pub(super) async fn account_summary(
    program: &Path,
    profile: Option<&str>,
    settings: &Settings,
) -> Result<Vec<Check>> {
    let projection = "SummaryMap.{AccountMFAEnabled:AccountMFAEnabled,AccountAccessKeysPresent:AccountAccessKeysPresent}";
    let value = match aws_json(
        program,
        profile,
        settings,
        "iam",
        "get-account-summary",
        &["--query", projection],
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            return credential_or_access(error, Stage::RootMfa, "iam get-account-summary").map(
                |result| match result {
                    Ok(check) | Err(check) => vec![check],
                },
            );
        }
    };
    let summary = super::parse::object(
        &value,
        &["AccountMFAEnabled", "AccountAccessKeysPresent"],
        "IAM GetAccountSummary",
    )?;
    let mfa = super::parse::integer(summary, "AccountMFAEnabled", "IAM GetAccountSummary")?;
    let keys = super::parse::integer(summary, "AccountAccessKeysPresent", "IAM GetAccountSummary")?;
    if mfa != 1 {
        return Ok(vec![action_required(
            Stage::RootMfa,
            "The AWS account summary does not report MFA enabled; enable MFA for the account root identity.",
        )]);
    }
    let mfa_check = verified(Stage::RootMfa, "AWS account summary reports MFA enabled.");
    if keys != 0 {
        return Ok(vec![
            mfa_check,
            action_required(
                Stage::RootAccessKeys,
                "The AWS account summary reports root access keys; remove the root access keys.",
            ),
        ]);
    }
    Ok(vec![
        mfa_check,
        verified(
            Stage::RootAccessKeys,
            "AWS account summary reports no access keys.",
        ),
    ])
}

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

pub(super) async fn identity_center(
    program: &Path,
    profile: Option<&str>,
    settings: &Settings,
) -> Result<std::result::Result<Check, Check>> {
    let query = "{Instances:Instances[].{InstanceArn:InstanceArn,IdentityStoreId:IdentityStoreId,OwnerAccountId:OwnerAccountId,Status:Status},NextToken:NextToken}";
    let value = match aws_json(
        program,
        profile,
        settings,
        "sso-admin",
        "list-instances",
        &["--query", query],
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            return credential_or_access(error, Stage::IdentityCenter, "sso-admin list-instances");
        }
    };
    let outer = super::parse::object(
        &value,
        &["Instances", "NextToken"],
        "SSO Admin ListInstances",
    )?;
    if outer.get("NextToken").is_some_and(|token| !token.is_null()) {
        return Ok(Err(blocked(
            Stage::IdentityCenter,
            "IAM Identity Center returned a continuation token; the instance list was incomplete and cannot establish a unique instance.",
        )));
    }
    let instances = super::parse::array(outer, "Instances", "SSO Admin ListInstances")?;
    if instances.is_empty() {
        return Ok(Err(action_required(
            Stage::IdentityCenter,
            "No IAM Identity Center organization instance is active in this region; enable it in the AWS console.",
        )));
    }
    if instances.len() != 1 {
        return Ok(Err(blocked(
            Stage::IdentityCenter,
            "More than one IAM Identity Center instance was returned; resolve the ambiguity in the AWS console.",
        )));
    }
    let instance = super::parse::object_value(
        instances.first(),
        &["InstanceArn", "IdentityStoreId", "OwnerAccountId", "Status"],
        "SSO Admin instance",
    )?;
    let owner = super::parse::string(instance, "OwnerAccountId", "SSO Admin instance")?;
    let status = super::parse::string(instance, "Status", "SSO Admin instance")?;
    if owner != settings.aws_account_id || status != "ACTIVE" {
        return Ok(Err(blocked(
            Stage::IdentityCenter,
            "The IAM Identity Center instance is not active and owned by the configured account.",
        )));
    }
    let arn = super::parse::string(instance, "InstanceArn", "SSO Admin instance")?;
    let store = super::parse::string(instance, "IdentityStoreId", "SSO Admin instance")?;
    if !valid_instance_arn(arn) || !valid_identity_store_id(store) {
        return Err(Error::state_failed(
            "SSO Admin ListInstances returned invalid instance identifiers",
        ));
    }
    Ok(Ok(verified(
        Stage::IdentityCenter,
        "Exactly one active IAM Identity Center instance is owned by the configured account.",
    )))
}

fn valid_instance_arn(arn: &str) -> bool {
    let Some((prefix, identifier)) = arn.split_once(":sso:::instance/") else {
        return false;
    };
    let Some(partition) = prefix.strip_prefix("arn:") else {
        return false;
    };
    matches!(partition, "aws" | "aws-us-gov" | "aws-cn")
        && identifier
            .strip_prefix("ssoins-")
            .is_some_and(valid_instance_identifier)
}

fn valid_identity_store_id(identifier: &str) -> bool {
    identifier.strip_prefix("d-").is_some_and(|value| {
        value.len() == 10 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) || valid_uuid(identifier)
}

fn valid_instance_identifier(identifier: &str) -> bool {
    (identifier.len() == 16 && identifier.bytes().all(|byte| byte.is_ascii_hexdigit()))
        || valid_uuid(identifier)
}

fn valid_uuid(value: &str) -> bool {
    let mut parts = value.split('-');
    [8, 4, 4, 4, 12].into_iter().all(|length| {
        parts.next().is_some_and(|part| {
            part.len() == length && part.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    }) && parts.next().is_none()
}

#[cfg(test)]
mod identifier_tests {
    use super::{valid_identity_store_id, valid_instance_arn};

    #[test]
    fn identity_store_ids_accept_supported_shapes_and_reject_prefixes() {
        assert!(valid_identity_store_id("d-1234567890"));
        assert!(valid_identity_store_id(
            "12345678-1234-abcd-9876-1234567890ab"
        ));
        assert!(!valid_identity_store_id("d-"));
        assert!(!valid_identity_store_id("d-123"));
        assert!(!valid_identity_store_id("d-123456789g"));
        assert!(!valid_identity_store_id(
            "12345678-1234-abcd-9876-1234567890ax"
        ));
    }

    #[test]
    fn identity_center_instance_arns_require_a_complete_identifier() {
        assert!(valid_instance_arn(
            "arn:aws:sso:::instance/ssoins-1234567890abcdef"
        ));
        assert!(valid_instance_arn(
            "arn:aws-us-gov:sso:::instance/ssoins-12345678-1234-abcd-9876-1234567890ab"
        ));
        assert!(!valid_instance_arn("arn:aws:sso:::instance/ssoins-"));
        assert!(!valid_instance_arn("arn:aws:sso:::instance/ssoins-invalid"));
        assert!(!valid_instance_arn(
            "arn:evil:sso:::instance/ssoins-1234567890abcdef"
        ));
        assert!(!valid_instance_arn(
            "arn:aws:sso:us-east-1:123456789012:instance/ssoins-1234567890abcdef"
        ));
    }
}
