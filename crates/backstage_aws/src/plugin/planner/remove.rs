use super::super::{catalog, dependencies, packages, result, source_edits};
use super::{EditPlan, push_unique, record_change};
use crate::{
    app_config,
    error::{Error, Result},
};
use std::path::Path;

pub(in crate::plugin) fn compute_remove(
    app: &Path,
    entries: &[&'static catalog::CatalogEntry],
    packages: &[packages::PackageRequest],
) -> Result<EditPlan> {
    let mut plan = EditPlan::default();
    remove_package_changes(app, packages, &mut plan)?;
    remove_catalog_changes(app, entries, &mut plan)?;
    report_removals(app, entries, packages, &mut plan)?;
    Ok(plan)
}

fn remove_package_changes(
    app: &Path,
    packages: &[packages::PackageRequest],
    plan: &mut EditPlan,
) -> Result<()> {
    let frontend_specs = packages
        .iter()
        .filter(|package| package.side == catalog::Side::Frontend)
        .collect::<Vec<_>>();
    if !frontend_specs.is_empty() {
        let path = app.join("packages/app/package.json");
        let source = result::read_text(&path)?;
        let updated = frontend_specs
            .iter()
            .try_fold(source.clone(), |current, package| {
                dependencies::remove_dependency(&current, &package.name)
            })?;
        record_change(plan, path, "app/packages/app/package.json", source, updated);
    }
    let backend_specs = packages
        .iter()
        .filter(|package| package.side == catalog::Side::Backend)
        .collect::<Vec<_>>();
    if !backend_specs.is_empty() {
        remove_backend_changes(app, &backend_specs, plan)?;
    }
    Ok(())
}

fn remove_backend_changes(
    app: &Path,
    packages: &[&packages::PackageRequest],
    plan: &mut EditPlan,
) -> Result<()> {
    let manifest_path = app.join("packages/backend/package.json");
    let manifest = result::read_text(&manifest_path)?;
    let updated_manifest = packages
        .iter()
        .try_fold(manifest.clone(), |current, package| {
            dependencies::remove_dependency(&current, &package.name)
        })?;
    record_change(
        plan,
        manifest_path,
        "app/packages/backend/package.json",
        manifest,
        updated_manifest,
    );
    let index_path = app.join("packages/backend/src/index.ts");
    let index = result::read_text(&index_path)?;
    let updated_index = packages
        .iter()
        .try_fold(index.clone(), |current, package| {
            source_edits::remove_backend_registration(&current, &package.name)
        })?;
    record_change(
        plan,
        index_path,
        "app/packages/backend/src/index.ts",
        index,
        updated_index,
    );
    Ok(())
}

fn remove_catalog_changes(
    app: &Path,
    entries: &[&'static catalog::CatalogEntry],
    plan: &mut EditPlan,
) -> Result<()> {
    if entries.iter().any(|entry| entry.id == "notifications") {
        remove_notifications_sidebar(app, plan)?;
    }
    if entries.iter().any(|entry| entry.id == "catalog-github") {
        remove_github_config(app, plan)?;
    }
    Ok(())
}

fn remove_notifications_sidebar(app: &Path, plan: &mut EditPlan) -> Result<()> {
    let path = app.join("packages/app/src/modules/nav/Sidebar.tsx");
    if let Some(source) = result::read_optional_text(&path)? {
        let updated = source_edits::remove_notifications_sidebar(&source)?;
        record_change(
            plan,
            path,
            "app/packages/app/src/modules/nav/Sidebar.tsx",
            source,
            updated,
        );
    }
    Ok(())
}

fn remove_github_config(app: &Path, plan: &mut EditPlan) -> Result<()> {
    let path = app.join("app-config.yaml");
    if let Some(source) = result::read_optional_text(&path)? {
        let updated = [
            "catalog.providers.github.main.organization",
            "catalog.providers.github.main.catalogPath",
            "catalog.providers.github.main.filters.branch",
            "catalog.providers.github.main.schedule.frequency.minutes",
            "catalog.providers.github.main.schedule.timeout.minutes",
        ]
        .iter()
        .try_fold(source.clone(), |current, key| {
            app_config::remove_text(&current, key)
        })?;
        record_change(plan, path, "app/app-config.yaml", source, updated);
    }
    Ok(())
}

fn report_removals(
    app: &Path,
    entries: &[&'static catalog::CatalogEntry],
    packages: &[packages::PackageRequest],
    plan: &mut EditPlan,
) -> Result<()> {
    packages.iter().try_for_each(|package| {
        let manifest_path = match package.side {
            catalog::Side::Frontend => app.join("packages/app/package.json"),
            catalog::Side::Backend => app.join("packages/backend/package.json"),
        };
        let source = result::read_text(&manifest_path)?;
        let present = dependencies::dependency_version(&source, &package.name)?.is_some();
        push_unique(
            if present {
                &mut plan.removed
            } else {
                &mut plan.skipped
            },
            package.name.clone(),
        );
        Ok::<(), Error>(())
    })?;
    entries.iter().try_for_each(|entry| {
        push_unique(&mut plan.removed, entry.id.to_owned());
        Ok::<(), Error>(())
    })
}
