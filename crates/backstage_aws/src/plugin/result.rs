use super::planner::{EditPlan, FileChange};
use crate::{
    error::{Error, Result},
    process,
};
use serde_json::{Value, json};
use std::{ffi::OsStr, path::Path, time::Duration};

pub(super) fn read_text(path: &Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|error| {
        Error::precondition_failed(format!("cannot read {}: {error}", path.display()))
    })
}

pub(super) fn read_optional_text(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(source) => Ok(Some(source)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::precondition_failed(format!(
            "cannot read {}: {error}",
            path.display()
        ))),
    }
}

pub(super) async fn finish(
    app: &Path,
    plan: EditPlan,
    dry_run: bool,
    install: bool,
) -> Result<Value> {
    let changed = plan.files.iter().any(|file| file.before != file.after);
    let file_results = plan
        .files
        .iter()
        .map(|file| {
            json!({
                "path": file.relative,
                "change": if file.before == file.after { "unchanged" } else { "modified" },
            })
        })
        .collect::<Vec<_>>();
    if !dry_run {
        for file in &plan.files {
            if file.before != file.after {
                write_file(file)?;
            }
        }
        if install {
            process::run_with_env(
                OsStr::new("corepack"),
                &["yarn", "install", "--no-immutable"],
                app,
                None,
                Duration::from_mins(45),
                &[],
            )
            .await
            .map_err(|error| Error::tool_failed("application", format!("yarn install: {error}")))?;
        }
    }
    let mut next = Vec::new();
    if dry_run && changed {
        next.push("re-run the command without `--dry-run` to apply these edits".to_owned());
    }
    let has_package_edit = plan.files.iter().any(|file| {
        (file.relative == "app/packages/app/package.json"
            || file.relative == "app/packages/backend/package.json")
            && file.before != file.after
    });
    if !dry_run && changed && has_package_edit && !install {
        next.push(
            "run `corepack yarn install --no-immutable` in app/ (or repeat with `--install`)"
                .to_owned(),
        );
    }
    if !dry_run && changed {
        next.push("restart with `backstage-aws dev` to use the updated plugin set".to_owned());
    }
    let added = plan.added;
    let skipped = plan.skipped;
    let removed = plan.removed;
    Ok(json!({
        "app": app.display().to_string(),
        "dry_run": dry_run,
        "changed": changed,
        "files": file_results,
        "added": added,
        "removed": removed,
        "skipped": skipped,
        "next": next,
    }))
}

fn write_file(file: &FileChange) -> Result<()> {
    std::fs::write(&file.path, &file.after).map_err(|error| {
        Error::state_failed(format!("cannot write {}: {error}", file.path.display()))
    })
}
