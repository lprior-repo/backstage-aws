//! Offline Backstage plugin management for a project's local `app/` tree.
use clap::Args;

mod catalog;
mod dependencies;
mod operations;
mod packages;
mod planner;
mod result;
mod source_edits;

#[cfg(test)]
#[path = "plugin/tests.rs"]
mod tests;

pub use operations::{add, list, remove};

/// Plugin IDs and package edits to apply to the local Backstage app.
#[derive(Args, Clone, Debug, Default)]
pub struct EditArgs {
    /// Catalog plugin names (see `backstage-aws plugin list`). May be repeated.
    #[arg(value_name = "PLUGIN")]
    pub(crate) ids: Vec<String>,
    /// Add a frontend package, optionally with an exact version (`PKG@VER`). Repeatable.
    #[arg(long = "frontend", value_name = "PKG[@VER]")]
    pub(crate) frontend: Vec<String>,
    /// Add a backend package, optionally with an exact version (`PKG@VER`). Repeatable.
    #[arg(long = "backend", value_name = "PKG[@VER]")]
    pub(crate) backend: Vec<String>,
    /// Set a dotted app-config.yaml path to a scalar value. Add only; repeatable as `KEY=VALUE`.
    #[arg(long = "set", value_name = "KEY=VALUE")]
    pub(crate) set: Vec<String>,
    /// Supply a catalog plugin parameter such as `org=example`. Repeatable as `KEY=VALUE`.
    #[arg(long = "param", value_name = "KEY=VALUE")]
    pub(crate) param: Vec<String>,
    /// Run `corepack yarn install --no-immutable` in app/ after applying the edits.
    #[arg(long)]
    pub(crate) install: bool,
    /// Compute and report all edits without writing files or running yarn.
    #[arg(long)]
    pub(crate) dry_run: bool,
}
