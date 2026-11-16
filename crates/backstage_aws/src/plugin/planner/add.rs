use super::super::{catalog, dependencies, packages, result, source_edits};
use super::{EditPlan, push_unique, record_change};
use crate::{
    app_config,
    error::{Error, Result},
};
use std::path::Path;

pub(in crate::plugin) fn compute_add(
    app: &Path,
    entries: &[&'static catalog::CatalogEntry],
    packages: &[packages::PackageSpec],
    parameters: &[(String, String)],
    settings: &[(String, String)],
) -> Result<EditPlan> {
    let frontend_specs = packages
        .iter()
        .filter(|package| package.side == catalog::Side::Frontend)
        .collect::<Vec<_>>();
    let backend_specs = packages
        .iter()
        .filter(|package| package.side == catalog::Side::Backend)
        .collect::<Vec<_>>();
    let frontend_source = if frontend_specs.is_empty() {
        None
    } else {
        Some(result::read_text(&app.join("packages/app/package.json"))?)
    };
    let backend_source = if backend_specs.is_empty() {
        None
    } else {
        Some(result::read_text(
            &app.join("packages/backend/package.json"),
        )?)
    };
    let index_source = if backend_specs.is_empty() {
        None
    } else {
        Some(result::read_text(
            &app.join("packages/backend/src/index.ts"),
        )?)
    };
    let sidebar_source = if entries.iter().any(|entry| entry.id == "notifications") {
        Some(result::read_text(
            &app.join("packages/app/src/modules/nav/Sidebar.tsx"),
        )?)
    } else {
        None
    };
    let mut plan = EditPlan::default();
    report_additions(
        &mut plan,
        entries,
        packages,
        frontend_source.as_deref(),
        backend_source.as_deref(),
        index_source.as_deref(),
        sidebar_source.as_deref(),
    )?;
    if let Some(source) = frontend_source {
        let updated = frontend_specs
            .iter()
            .try_fold(source.clone(), |current, package| {
                dependencies::set_dependency(&current, &package.name, &package.version)
            })?;
        record_change(
            &mut plan,
            app.join("packages/app/package.json"),
            "app/packages/app/package.json",
            source,
            updated,
        );
    }
    if let Some(source) = backend_source {
        let updated = backend_specs
            .iter()
            .try_fold(source.clone(), |current, package| {
                dependencies::set_dependency(&current, &package.name, &package.version)
            })?;
        record_change(
            &mut plan,
            app.join("packages/backend/package.json"),
            "app/packages/backend/package.json",
            source,
            updated,
        );
    }
    if let Some(source) = index_source {
        let updated = backend_specs
            .iter()
            .try_fold(source.clone(), |current, package| {
                source_edits::add_backend_registration(&current, &package.name)
            })?;
        record_change(
            &mut plan,
            app.join("packages/backend/src/index.ts"),
            "app/packages/backend/src/index.ts",
            source,
            updated,
        );
    }
    if let Some(source) = sidebar_source {
        let updated = source_edits::add_notifications_sidebar(&source)?;
        record_change(
            &mut plan,
            app.join("packages/app/src/modules/nav/Sidebar.tsx"),
            "app/packages/app/src/modules/nav/Sidebar.tsx",
            source,
            updated,
        );
    }
    add_config_changes(app, entries, parameters, settings, &mut plan)?;
    Ok(plan)
}

fn add_config_changes(
    app: &Path,
    entries: &[&'static catalog::CatalogEntry],
    parameters: &[(String, String)],
    settings: &[(String, String)],
    plan: &mut EditPlan,
) -> Result<()> {
    let catalog_github = entries.iter().any(|entry| entry.id == "catalog-github");
    if !catalog_github && settings.is_empty() {
        return Ok(());
    }
    let path = app.join("app-config.yaml");
    let source = result::read_text(&path)?;
    let mut updated = source.clone();
    if catalog_github {
        let organization = parameters
            .iter()
            .find(|(key, _)| key == "org")
            .map(|(_, value)| value.as_str())
            .ok_or_else(|| {
                Error::invalid_input("catalog-github requires `--param org=ORGANIZATION`")
            })?;
        [
            ("catalog.providers.github.main.organization", organization),
            (
                "catalog.providers.github.main.catalogPath",
                "/catalog-info.yaml",
            ),
            ("catalog.providers.github.main.filters.branch", "main"),
            (
                "catalog.providers.github.main.schedule.frequency.minutes",
                "30",
            ),
            (
                "catalog.providers.github.main.schedule.timeout.minutes",
                "3",
            ),
        ]
        .iter()
        .try_for_each(|(key, value)| {
            updated = app_config::set_text(&updated, key, value)?;
            Ok::<(), Error>(())
        })?;
    }
    settings.iter().try_for_each(|(key, value)| {
        updated = app_config::set_text(&updated, key, value)?;
        Ok::<(), Error>(())
    })?;
    record_change(plan, path, "app/app-config.yaml", source, updated);
    Ok(())
}

fn report_additions(
    plan: &mut EditPlan,
    entries: &[&'static catalog::CatalogEntry],
    packages: &[packages::PackageSpec],
    frontend: Option<&str>,
    backend: Option<&str>,
    backend_index: Option<&str>,
    sidebar: Option<&str>,
) -> Result<()> {
    entries.iter().try_for_each(|entry| {
        let installed = entry_installed(entry, frontend, backend, backend_index, sidebar)?;
        push_unique(
            if installed {
                &mut plan.skipped
            } else {
                &mut plan.added
            },
            entry.id.to_owned(),
        );
        Ok(())
    })?;
    packages.iter().try_for_each(|package| {
        let manifest = match package.side {
            catalog::Side::Frontend => frontend,
            catalog::Side::Backend => backend,
        };
        let version_matches = match manifest {
            Some(source) => {
                dependencies::dependency_version(source, &package.name)?.as_deref()
                    == Some(package.version.as_str())
            }
            None => false,
        };
        let registration_matches = if package.side == catalog::Side::Backend {
            match backend_index {
                Some(source) => source_edits::backend_has_registration(source, &package.name)?,
                None => false,
            }
        } else {
            true
        };
        push_unique(
            if version_matches && registration_matches {
                &mut plan.skipped
            } else {
                &mut plan.added
            },
            package.name.clone(),
        );
        Ok(())
    })
}

fn entry_installed(
    entry: &catalog::CatalogEntry,
    frontend: Option<&str>,
    backend: Option<&str>,
    backend_index: Option<&str>,
    sidebar: Option<&str>,
) -> Result<bool> {
    let frontend_present = entry.frontend.iter().try_fold(true, |present, package| {
        let installed = frontend
            .map(|source| dependencies::dependency_version(source, package))
            .transpose()?
            .is_some_and(|version| version.is_some());
        Ok::<bool, Error>(present && installed)
    })?;
    let backend_present = entry.backend.iter().try_fold(true, |present, package| {
        let dependency = backend
            .map(|source| dependencies::dependency_version(source, package))
            .transpose()?
            .is_some_and(|version| version.is_some());
        let registered = backend_index
            .map(|source| source_edits::backend_has_registration(source, package))
            .transpose()?
            .unwrap_or(false);
        Ok::<bool, Error>(present && dependency && registered)
    })?;
    let sidebar_present = entry.id != "notifications"
        || sidebar.is_some_and(source_edits::notifications_sidebar_installed);
    Ok(frontend_present && backend_present && sidebar_present)
}
