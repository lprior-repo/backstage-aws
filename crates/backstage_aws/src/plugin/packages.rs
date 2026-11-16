use super::{
    EditArgs,
    catalog::{CatalogEntry, Side},
};
use crate::error::{Error, Result};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug)]
pub(super) struct PackageRequest {
    pub side: Side,
    pub name: String,
    pub version: Option<String>,
}

#[derive(Clone, Debug)]
pub(super) struct PackageSpec {
    pub side: Side,
    pub name: String,
    pub version: String,
}

pub(super) fn requests(
    args: &EditArgs,
    entries: &[&'static CatalogEntry],
) -> Result<Vec<PackageRequest>> {
    let mut packages: BTreeMap<(Side, String), Option<String>> = BTreeMap::new();
    entries.iter().try_for_each(|entry| {
        entry
            .frontend
            .iter()
            .try_for_each(|name| merge_package(&mut packages, Side::Frontend, name, None))?;
        entry
            .backend
            .iter()
            .try_for_each(|name| merge_package(&mut packages, Side::Backend, name, None))
    })?;
    args.frontend.iter().try_for_each(|input| {
        let (name, version) = parse_package(input)?;
        merge_package(&mut packages, Side::Frontend, &name, version)
    })?;
    args.backend.iter().try_for_each(|input| {
        let (name, version) = parse_package(input)?;
        merge_package(&mut packages, Side::Backend, &name, version)
    })?;
    Ok(packages
        .into_iter()
        .map(|((side, name), version)| PackageRequest {
            side,
            name,
            version,
        })
        .collect())
}

fn merge_package(
    packages: &mut BTreeMap<(Side, String), Option<String>>,
    side: Side,
    name: &str,
    version: Option<String>,
) -> Result<()> {
    let key = (side, name.to_owned());
    if let Some(existing) = packages.get_mut(&key) {
        match (existing.as_deref(), version.as_deref()) {
            (Some(current), Some(requested)) if current != requested => Err(Error::invalid_input(
                format!("conflicting versions requested for '{name}': {current} and {requested}"),
            )),
            (None, Some(_)) => {
                *existing = version;
                Ok(())
            }
            _ => Ok(()),
        }
    } else {
        packages.insert(key, version);
        Ok(())
    }
}

fn parse_package(input: &str) -> Result<(String, Option<String>)> {
    let (name, version) = match input.rsplit_once('@') {
        Some((name, version)) if !name.is_empty() => (name, Some(version)),
        _ => (input, None),
    };
    if !valid_package_name(name) {
        return Err(Error::invalid_input(format!(
            "invalid package name '{name}'"
        )));
    }
    let version = version
        .map(|value| {
            if is_exact_semver(value) {
                Ok(value.to_owned())
            } else {
                Err(Error::invalid_input(format!(
                    "'{value}' is not an exact semantic version; use PKG@MAJOR.MINOR.PATCH"
                )))
            }
        })
        .transpose()?;
    Ok((name.to_owned(), version))
}

fn valid_package_name(name: &str) -> bool {
    let (scope, package) = match name.strip_prefix('@') {
        Some(rest) => match rest.split_once('/') {
            Some((scope, package)) => (Some(scope), package),
            None => return false,
        },
        None => (None, name),
    };
    let valid_segment = |segment: &str| {
        !segment.is_empty()
            && segment.chars().all(|character| {
                character.is_ascii_lowercase()
                    || character.is_ascii_digit()
                    || matches!(character, '.' | '_' | '-')
            })
    };
    scope.is_none_or(valid_segment) && valid_segment(package)
}

pub(super) fn is_exact_semver(version: &str) -> bool {
    let (without_build, build) = match version.split_once('+') {
        Some((left, right)) if !right.contains('+') => (left, Some(right)),
        Some(_) => return false,
        None => (version, None),
    };
    if build.is_some_and(|metadata| !valid_identifiers(metadata, false)) {
        return false;
    }
    let (core, prerelease) = match without_build.split_once('-') {
        Some((left, right)) => (left, Some(right)),
        None => (without_build, None),
    };
    if prerelease.is_some_and(|identifiers| !valid_identifiers(identifiers, true)) {
        return false;
    }
    let mut parts = core.split('.');
    parts.next().is_some_and(valid_numeric_identifier)
        && parts.next().is_some_and(valid_numeric_identifier)
        && parts.next().is_some_and(valid_numeric_identifier)
        && parts.next().is_none()
}

fn valid_identifiers(value: &str, prerelease: bool) -> bool {
    !value.is_empty()
        && value.split('.').all(|identifier| {
            !identifier.is_empty()
                && identifier
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
                && (!prerelease
                    || !is_numeric_identifier(identifier)
                    || identifier == "0"
                    || !identifier.starts_with('0'))
        })
}

fn valid_numeric_identifier(value: &str) -> bool {
    is_numeric_identifier(value) && (value == "0" || !value.starts_with('0'))
}

fn is_numeric_identifier(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|character| character.is_ascii_digit())
}

pub(super) fn resolve_add(app: &Path, requests: Vec<PackageRequest>) -> Result<Vec<PackageSpec>> {
    if requests.is_empty() {
        return Ok(Vec::new());
    }
    let release = requests
        .iter()
        .any(|request| request.version.is_none())
        .then(|| pinned_versions(app))
        .transpose()?;
    requests
        .into_iter()
        .map(|request| {
            let version = match request.version {
                Some(version) => version,
                None => release
                    .as_ref()
                    .and_then(|pinned| pinned.get(&request.name))
                    .cloned()
                    .ok_or_else(|| {
                        Error::invalid_input(format!(
                            "no pinned version is known for '{}'; specify it as {}@VER",
                            request.name, request.name
                        ))
                    })?,
            };
            if !is_exact_semver(&version) {
                return Err(Error::state_failed(format!(
                    "pinned version '{version}' for '{}' is not an exact semantic version",
                    request.name
                )));
            }
            Ok(PackageSpec {
                side: request.side,
                name: request.name,
                version,
            })
        })
        .collect()
}

fn pinned_versions(app: &Path) -> Result<BTreeMap<String, String>> {
    let backstage_path = app.join("backstage.json");
    let backstage_text = std::fs::read_to_string(&backstage_path).map_err(|error| {
        Error::precondition_failed(format!("cannot read {}: {error}", backstage_path.display()))
    })?;
    let backstage: Value = serde_json::from_str(&backstage_text).map_err(|error| {
        Error::state_failed(format!("invalid {}: {error}", backstage_path.display()))
    })?;
    let release_version = backstage
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::state_failed(format!(
                "{} has no string version",
                backstage_path.display()
            ))
        })?;
    if !is_exact_semver(release_version) {
        return Err(Error::state_failed(format!(
            "{} has an invalid Backstage release version '{release_version}'",
            backstage_path.display()
        )));
    }
    let release_path = app
        .join("provenance")
        .join(format!("backstage-release-{release_version}.json"));
    let release_text = std::fs::read_to_string(&release_path).map_err(|error| {
        Error::precondition_failed(format!(
            "cannot read pinned release metadata {}: {error}",
            release_path.display()
        ))
    })?;
    let release: Value = serde_json::from_str(&release_text).map_err(|error| {
        Error::state_failed(format!("invalid {}: {error}", release_path.display()))
    })?;
    let packages = release
        .get("packages")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            Error::state_failed(format!("{} has no packages array", release_path.display()))
        })?;
    packages
        .iter()
        .try_fold(BTreeMap::new(), |mut versions, package| {
            let name = package.get("name").and_then(Value::as_str).ok_or_else(|| {
                Error::state_failed("release metadata contains a package without a name")
            })?;
            let version = package
                .get("version")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Error::state_failed(format!("release metadata package '{name}' has no version"))
                })?;
            if versions
                .insert(name.to_owned(), version.to_owned())
                .is_some()
            {
                return Err(Error::state_failed(format!(
                    "release metadata contains duplicate package '{name}'"
                )));
            }
            Ok(versions)
        })
}
