use super::{EditArgs, catalog, packages, planner, result};
use crate::error::{Error, Result};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub fn list(project: &Path) -> Result<Value> {
    let app = app_dir(project)?;
    let frontend = result::read_optional_text(&app.join("packages/app/package.json"))?;
    let backend = result::read_optional_text(&app.join("packages/backend/package.json"))?;
    let backend_index = result::read_optional_text(&app.join("packages/backend/src/index.ts"))?;
    let sidebar =
        result::read_optional_text(&app.join("packages/app/src/modules/nav/Sidebar.tsx"))?;
    let plugins = catalog::CATALOG
        .iter()
        .map(|entry| {
            let installed = installed(
                entry,
                frontend.as_deref(),
                backend.as_deref(),
                backend_index.as_deref(),
                sidebar.as_deref(),
            )?;
            Ok(json!({
                "id": entry.id,
                "summary": entry.summary,
                "installed": installed,
                "frontend": entry.frontend,
                "backend": entry.backend,
                "requires": entry.requires,
            }))
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({ "plugins": plugins }))
}

pub async fn add(project: &Path, args: &EditArgs) -> Result<Value> {
    let app = app_dir(project)?;
    ensure_selection(args)?;
    let entries = catalog::entries_for(args, true)?;
    let parameters = parse_pairs(&args.param, "--param")?;
    validate_parameters(&entries, &parameters)?;
    let settings = parse_pairs(&args.set, "--set")?;
    let requests = packages::requests(args, &entries)?;
    let resolved = packages::resolve_add(&app, requests)?;
    let plan = planner::compute_add(&app, &entries, &resolved, &parameters, &settings)?;
    result::finish(&app, plan, args.dry_run, args.install).await
}

pub async fn remove(project: &Path, args: &EditArgs) -> Result<Value> {
    let app = app_dir(project)?;
    ensure_selection(args)?;
    if !args.set.is_empty() {
        return Err(Error::invalid_input(
            "--set is only supported by `plugin add`; remove configuration by removing its catalog plugin",
        ));
    }
    let entries = catalog::entries_for(args, false)?;
    let parameters = parse_pairs(&args.param, "--param")?;
    let github = entries.iter().any(|entry| entry.id == "catalog-github");
    if !parameters.is_empty() && !github {
        return Err(Error::invalid_input(
            "--param is only supported for the catalog-github plugin",
        ));
    }
    parameters.iter().try_for_each(|(key, _)| {
        if key != "org" {
            return Err(Error::invalid_input(format!(
                "unknown catalog-github parameter '{key}'; supported parameter: org"
            )));
        }
        Ok(())
    })?;
    let requests = packages::requests(args, &entries)?;
    let plan = planner::compute_remove(&app, &entries, &requests)?;
    result::finish(&app, plan, args.dry_run, args.install).await
}

fn ensure_selection(args: &EditArgs) -> Result<()> {
    if args.ids.is_empty()
        && args.frontend.is_empty()
        && args.backend.is_empty()
        && args.set.is_empty()
    {
        return Err(Error::invalid_input(
            "specify a plugin ID, --frontend package, --backend package, or --set key=value",
        ));
    }
    Ok(())
}

fn app_dir(project: &Path) -> Result<PathBuf> {
    let app = project.join("app");
    if !app.join("package.json").is_file() {
        return Err(Error::precondition_failed(format!(
            "{} is missing app/package.json; initialize the project with `backstage-aws init` first",
            project.display()
        )));
    }
    Ok(app)
}

fn parse_pairs(values: &[String], flag: &str) -> Result<Vec<(String, String)>> {
    let mut pairs = BTreeMap::new();
    values.iter().try_for_each(|value| {
        let (key, value) = value.split_once('=').ok_or_else(|| {
            Error::invalid_input(format!("{flag} expects KEY=VALUE, got '{value}'"))
        })?;
        if key.is_empty() {
            return Err(Error::invalid_input(format!("{flag} key cannot be empty")));
        }
        pairs.insert(key.to_owned(), value.to_owned());
        Ok(())
    })?;
    Ok(pairs.into_iter().collect())
}

fn validate_parameters(
    entries: &[&'static catalog::CatalogEntry],
    parameters: &[(String, String)],
) -> Result<()> {
    let github = entries.iter().any(|entry| entry.id == "catalog-github");
    if !github && !parameters.is_empty() {
        return Err(Error::invalid_input(
            "--param is only supported for the catalog-github plugin",
        ));
    }
    parameters.iter().try_for_each(|(key, _)| {
        if github && key != "org" {
            return Err(Error::invalid_input(format!(
                "unknown catalog-github parameter '{key}'; supported parameter: org"
            )));
        }
        Ok(())
    })?;
    if github
        && !parameters
            .iter()
            .any(|(key, value)| key == "org" && !value.is_empty())
    {
        return Err(Error::invalid_input(
            "catalog-github requires `--param org=ORGANIZATION`",
        ));
    }
    Ok(())
}

fn installed(
    entry: &catalog::CatalogEntry,
    frontend: Option<&str>,
    backend: Option<&str>,
    backend_index: Option<&str>,
    sidebar: Option<&str>,
) -> Result<bool> {
    let frontend_present = entry.frontend.iter().try_fold(true, |present, package| {
        let has = frontend
            .map(|source| super::dependencies::dependency_version(source, package))
            .transpose()?
            .is_some_and(|version| version.is_some());
        Ok::<bool, Error>(present && has)
    })?;
    let backend_present = entry.backend.iter().try_fold(true, |present, package| {
        let dependency = backend
            .map(|source| super::dependencies::dependency_version(source, package))
            .transpose()?
            .is_some_and(|version| version.is_some());
        let registered = match backend_index {
            Some(source) => super::source_edits::backend_has_registration(source, package)?,
            None => false,
        };
        Ok::<bool, Error>(present && dependency && registered)
    })?;
    let sidebar_present = entry.id != "notifications"
        || sidebar.is_some_and(super::source_edits::notifications_sidebar_installed);
    Ok(frontend_present && backend_present && sidebar_present)
}
