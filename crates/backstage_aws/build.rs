//! Stages the project template for embedding.
//!
//! `include_dir!` embeds every file under its directory and reports none of
//! them to Cargo. Pointed at `template/` itself it ships whatever a local run
//! left there (`node_modules`, `.terraform`, state files) and keeps a stale
//! copy after an edit. Staging only what the template's own `.gitignore`
//! rules keep embeds exactly the files a generated project would commit.
use std::{env, error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    let source = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?).join("../../template");
    let staged = PathBuf::from(env::var("OUT_DIR")?).join("template");
    println!("cargo:rerun-if-changed={}", source.display());
    if staged.exists() {
        fs::remove_dir_all(&staged)?;
    }
    // Only the template's own rules apply: the generated project is its own
    // repository root, so neither this repository's ignore files nor the
    // builder's global ones may change what is embedded.
    let entries = ignore::WalkBuilder::new(&source)
        .hidden(false)
        .parents(false)
        .ignore(false)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build();
    for entry in entries {
        let entry = entry?;
        let target = staged.join(entry.path().strip_prefix(&source)?);
        match entry.file_type() {
            Some(kind) if kind.is_dir() => fs::create_dir_all(&target)?,
            Some(kind) if kind.is_file() => {
                fs::copy(entry.path(), &target)?;
            }
            _ => {
                return Err(format!(
                    "{} is neither a file nor a directory; the template cannot embed it",
                    entry.path().display()
                )
                .into());
            }
        }
    }
    Ok(())
}
