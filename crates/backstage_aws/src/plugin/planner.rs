use std::path::PathBuf;

mod add;
mod remove;

pub(super) use add::compute_add;
pub(super) use remove::compute_remove;

#[derive(Debug)]
pub(super) struct FileChange {
    pub relative: String,
    pub path: PathBuf,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Default)]
pub(super) struct EditPlan {
    pub files: Vec<FileChange>,
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub skipped: Vec<String>,
}

pub(super) fn record_change(
    plan: &mut EditPlan,
    path: PathBuf,
    relative: &str,
    before: String,
    after: String,
) {
    if let Some(existing) = plan.files.iter_mut().find(|file| file.path == path) {
        existing.after = after;
    } else {
        plan.files.push(FileChange {
            relative: relative.to_owned(),
            path,
            before,
            after,
        });
    }
}

pub(super) fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}
