use super::EditArgs;
use crate::error::{Error, Result};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum Side {
    Frontend,
    Backend,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct CatalogEntry {
    pub id: &'static str,
    pub summary: &'static str,
    pub frontend: &'static [&'static str],
    pub backend: &'static [&'static str],
    pub requires: &'static [&'static str],
}

const SIGNALS_FRONTEND: &[&str] = &["@backstage/plugin-signals"];
const SIGNALS_BACKEND: &[&str] = &["@backstage/plugin-signals-backend"];
const NOTIFICATIONS_FRONTEND: &[&str] = &["@backstage/plugin-notifications"];
const NOTIFICATIONS_BACKEND: &[&str] = &["@backstage/plugin-notifications-backend"];
const API_DOCS: &[&str] = &["@backstage/plugin-api-docs"];
const CATALOG_GRAPH: &[&str] = &["@backstage/plugin-catalog-graph"];
const CATALOG_GITHUB: &[&str] = &["@backstage/plugin-catalog-backend-module-github"];
const SCAFFOLDER_GITHUB: &[&str] = &["@backstage/plugin-scaffolder-backend-module-github"];
const REQUIRES_SIGNALS: &[&str] = &["signals"];

pub(super) const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "signals",
        summary: "Add real-time notifications and signals support.",
        frontend: SIGNALS_FRONTEND,
        backend: SIGNALS_BACKEND,
        requires: &[],
    },
    CatalogEntry {
        id: "notifications",
        summary: "Add the notifications UI and backend, including its sidebar entry.",
        frontend: NOTIFICATIONS_FRONTEND,
        backend: NOTIFICATIONS_BACKEND,
        requires: REQUIRES_SIGNALS,
    },
    CatalogEntry {
        id: "api-docs",
        summary: "Add API documentation support to the catalog.",
        frontend: API_DOCS,
        backend: &[],
        requires: &[],
    },
    CatalogEntry {
        id: "catalog-graph",
        summary: "Add the catalog relationship graph plugin.",
        frontend: CATALOG_GRAPH,
        backend: &[],
        requires: &[],
    },
    CatalogEntry {
        id: "catalog-github",
        summary: "Load catalog entities from a GitHub organization repository.",
        frontend: &[],
        backend: CATALOG_GITHUB,
        requires: &[],
    },
    CatalogEntry {
        id: "scaffolder-github",
        summary: "Add GitHub actions to scaffolder backend templates.",
        frontend: &[],
        backend: SCAFFOLDER_GITHUB,
        requires: &[],
    },
];

pub(super) fn find_entry(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|entry| entry.id == id)
}

pub(super) fn entries_for(
    args: &EditArgs,
    include_requirements: bool,
) -> Result<Vec<&'static CatalogEntry>> {
    let mut entries = Vec::new();
    let mut ids = BTreeSet::new();
    args.ids.iter().try_for_each(|id| {
        let entry = find_entry(id).ok_or_else(|| {
            Error::invalid_input(format!(
                "unknown plugin '{id}'; run `backstage-aws plugin list` to see available plugins"
            ))
        })?;
        if ids.insert(entry.id) {
            entries.push(entry);
        }
        Ok(())
    })?;
    if include_requirements {
        args.ids
            .iter()
            .filter_map(|id| find_entry(id))
            .flat_map(|entry| entry.requires.iter())
            .try_for_each(|required| {
                let entry = find_entry(required).ok_or_else(|| {
                    Error::state_failed(format!("catalog requirement '{required}' is missing"))
                })?;
                if ids.insert(entry.id) {
                    entries.push(entry);
                }
                Ok(())
            })?;
    }
    Ok(entries)
}
