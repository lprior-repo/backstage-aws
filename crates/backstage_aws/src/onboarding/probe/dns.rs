use super::aws::credential_or_access;
use super::{Check, Stage};
use super::{action_required, aws_base, invoke, parse_output, verified};
use crate::{
    error::{Error, Result},
    settings::Settings,
};
use serde_json::Value;
use std::{collections::BTreeSet, path::Path};

pub(super) async fn delegation(
    program: &Path,
    profile: Option<&str>,
    settings: &Settings,
) -> Result<std::result::Result<Check, Check>> {
    let query = "{HostedZone:{Id:HostedZone.Id,Name:HostedZone.Name,Config:{PrivateZone:HostedZone.Config.PrivateZone}},DelegationSet:{NameServers:DelegationSet.NameServers}}";
    let mut argv = vec![
        "route53".into(),
        "get-hosted-zone".into(),
        "--id".into(),
        settings.route53_zone_id.clone(),
        "--query".into(),
        query.into(),
    ];
    argv.extend(aws_base(profile, &settings.aws_region));
    let output = invoke(program, &argv, Path::new(".")).await?;
    let value = match parse_output(&output, "Route 53", "GetHostedZone") {
        Ok(value) => value,
        Err(error) => {
            return credential_or_access(error, Stage::DnsDelegation, "route53 get-hosted-zone");
        }
    };
    if !public_zone(&value)? {
        return Ok(Err(action_required(
            Stage::DnsDelegation,
            "The configured Route 53 hosted zone is private; use a public hosted zone.",
        )));
    }
    let (zone_name, expected) = zone_data(&value, settings)?;
    if !domain_belongs(&settings.domain_name, &zone_name) {
        return Ok(Err(action_required(
            Stage::DnsDelegation,
            format!(
                "Configured domain {} is not the hosted zone {} or one of its subdomains.",
                settings.domain_name, zone_name
            ),
        )));
    }
    if expected.is_empty() {
        return Err(Error::state_failed(
            "Route 53 GetHostedZone returned no delegation name servers",
        ));
    }
    let dig = match invoke(
        Path::new("dig"),
        &[
            "@1.1.1.1".into(),
            "+short".into(),
            "+time=3".into(),
            "+tries=1".into(),
            "NS".into(),
            zone_name.clone(),
        ],
        Path::new("."),
    )
    .await
    {
        Ok(output) => output,
        Err(Error::ToolFailed { detail, .. })
            if detail.starts_with("could not start executable:") =>
        {
            return Ok(Err(action_required(
                Stage::DnsDelegation,
                "The `dig` executable is required to verify public DNS delegation; install it and retry.",
            )));
        }
        Err(error) => return Err(error),
    };
    if !dig.status.success() {
        return Err(Error::tool_failed("dig", "public NS lookup failed"));
    }
    let observed = nameservers(&dig.stdout)?;
    if observed.is_empty() || observed != expected {
        return Ok(Err(action_required(
            Stage::DnsDelegation,
            "Public DNS name servers do not match the configured Route 53 zone delegation.",
        )));
    }
    Ok(Ok(verified(
        Stage::DnsDelegation,
        "The public hosted zone and its delegated name servers match.",
    )))
}

fn zone_data(value: &Value, settings: &Settings) -> Result<(String, BTreeSet<String>)> {
    let outer = super::parse::object(
        value,
        &["HostedZone", "DelegationSet"],
        "Route 53 GetHostedZone",
    )?;
    let zone = super::parse::object(
        super::parse::value(outer, "HostedZone", "Route 53 GetHostedZone")?,
        &["Id", "Name", "Config"],
        "Route 53 hosted zone",
    )?;
    let id = super::parse::string(zone, "Id", "Route 53 hosted zone")?;
    if id.trim_start_matches("/hostedzone/")
        != settings.route53_zone_id.trim_start_matches("/hostedzone/")
    {
        return Err(Error::state_failed(
            "Route 53 GetHostedZone returned a different zone ID",
        ));
    }
    let name = normalize(super::parse::string(zone, "Name", "Route 53 hosted zone")?);
    let delegation = super::parse::object(
        super::parse::value(outer, "DelegationSet", "Route 53 GetHostedZone")?,
        &["NameServers"],
        "Route 53 delegation set",
    )?;
    let servers = super::parse::array(delegation, "NameServers", "Route 53 delegation set")?;
    let names = servers
        .iter()
        .map(|server| {
            server
                .as_str()
                .map(normalize)
                .ok_or_else(|| Error::state_failed("Route 53 delegation server was not a string"))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    Ok((name, names))
}

fn public_zone(value: &Value) -> Result<bool> {
    let outer = super::parse::object(
        value,
        &["HostedZone", "DelegationSet"],
        "Route 53 GetHostedZone",
    )?;
    let zone = super::parse::object(
        super::parse::value(outer, "HostedZone", "Route 53 GetHostedZone")?,
        &["Id", "Name", "Config"],
        "Route 53 hosted zone",
    )?;
    let config = super::parse::object(
        super::parse::value(zone, "Config", "Route 53 hosted zone")?,
        &["PrivateZone"],
        "Route 53 zone config",
    )?;
    super::parse::value(config, "PrivateZone", "Route 53 zone config")?
        .as_bool()
        .map(|private| !private)
        .ok_or_else(|| Error::state_failed("Route 53 PrivateZone was not a boolean"))
}

fn domain_belongs(domain: &str, zone: &str) -> bool {
    let domain = normalize(domain);
    let zone = normalize(zone);
    domain == zone || domain.strip_suffix(&format!(".{zone}")).is_some()
}

fn normalize(name: &str) -> String {
    name.trim_end_matches('.').to_ascii_lowercase()
}

fn nameservers(stdout: &[u8]) -> Result<BTreeSet<String>> {
    let text = std::str::from_utf8(stdout)
        .map_err(|_| Error::state_failed("dig returned non-UTF-8 name server output"))?;
    text.lines()
        .map(|line| {
            let name = normalize(line.trim());
            if !valid_dns_name(&name) {
                return Err(Error::state_failed(
                    "dig returned malformed name server output",
                ));
            }
            Ok(name)
        })
        .collect()
}

fn valid_dns_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|label| {
            !label.is_empty()
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dns_uses_label_boundaries_and_normalized_sets() {
        assert!(domain_belongs("app.example.com", "example.com"));
        assert!(!domain_belongs("example.com.attacker", "example.com"));
        let servers =
            nameservers(b"NS1.EXAMPLE.NET.\nns2.example.net\n").expect("name servers parse");
        assert_eq!(
            servers,
            ["ns1.example.net".to_owned(), "ns2.example.net".to_owned()]
                .into_iter()
                .collect()
        );
        assert!(nameservers(b"not a name server\n").is_err());
    }
}
