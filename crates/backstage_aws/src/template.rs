//! The project template, embedded in the binary and written out by `init`.
use crate::{
    error::{Error, Result},
    settings::ValidatedSettings,
};
use include_dir::{Dir, DirEntry, include_dir};
use serde::Serialize;
use std::{fs, path::Path};

static TEMPLATE: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../template");
const MARKER: &str = "__BACKSTAGE_AWS_";

#[derive(Serialize)]
pub(crate) struct InitResult {
    output: String,
    files: usize,
    settings_file: String,
}

/// Records the environment's settings in the project, where CI and operators
/// load them from. Returns the path relative to the project.
pub(crate) fn write_settings_file(settings: &ValidatedSettings, project: &Path) -> Result<String> {
    let failed = |error: std::io::Error| Error::state_failed(error.to_string());
    let relative = format!("environments/{}.env", settings.settings.environment);
    fs::create_dir_all(project.join("environments")).map_err(failed)?;
    fs::write(project.join(&relative), settings.settings_file()).map_err(failed)?;
    Ok(relative)
}

/// Writes the template into a new directory, substituting project values.
pub(crate) fn render_template(settings: &ValidatedSettings, output: &Path) -> Result<InitResult> {
    if output.exists() {
        return Err(Error::invalid_input(format!(
            "{} already exists; `init` only writes to a new directory",
            output.display()
        )));
    }
    let failed = |error: std::io::Error| Error::state_failed(error.to_string());
    let mut files = 0_usize;
    let mut pending = vec![&TEMPLATE];
    while let Some(directory) = pending.pop() {
        fs::create_dir_all(output.join(directory.path())).map_err(failed)?;
        for entry in directory.entries() {
            match entry {
                DirEntry::Dir(child) => pending.push(child),
                DirEntry::File(file) => {
                    let contents = render_contents(settings, file.contents())?;
                    fs::write(output.join(file.path()), contents).map_err(failed)?;
                    files = files.saturating_add(1);
                }
            }
        }
    }
    Ok(InitResult {
        output: output.display().to_string(),
        settings_file: write_settings_file(settings, output)?,
        files: files.saturating_add(1),
    })
}

/// Text files get their placeholders replaced; binary files pass through.
fn render_contents(settings: &ValidatedSettings, contents: &[u8]) -> Result<Vec<u8>> {
    let Ok(text) = std::str::from_utf8(contents) else {
        return Ok(contents.to_vec());
    };
    let rendered = text
        .replace("__BACKSTAGE_AWS_NAME__", &settings.settings.name)
        .replace(
            "__BACKSTAGE_AWS_OWNER_ID__",
            &settings.owner_github_id.0.to_string(),
        )
        .replace("__BACKSTAGE_AWS_CLI_VERSION__", env!("CARGO_PKG_VERSION"))
        .replace(
            "__BACKSTAGE_AWS_CLI_REPOSITORY__",
            "lprior-repo/backstage-aws",
        );
    if rendered.contains(MARKER) {
        return Err(Error::state_failed(
            "the embedded template contains an unknown placeholder",
        ));
    }
    Ok(rendered.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use std::path::PathBuf;

    type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

    fn files_under(root: &Path) -> std::io::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(directory) = pending.pop() {
            for entry in fs::read_dir(directory)? {
                let path = entry?.path();
                if path.is_dir() {
                    pending.push(path);
                } else {
                    files.push(path);
                }
            }
        }
        Ok(files)
    }

    #[test]
    fn init_writes_the_project_with_every_placeholder_resolved() -> TestResult {
        let temp = tempfile::tempdir()?;
        let output = temp.path().join("generated");
        let settings = Settings::sample(output.clone()).validate()?;
        let result = render_template(&settings, &output)?;
        let written = files_under(&output)?;
        assert_eq!(written.len(), result.files);
        for path in &written {
            let bytes = fs::read(path)?;
            let unresolved = bytes
                .windows(MARKER.len())
                .any(|window| window == MARKER.as_bytes());
            assert!(!unresolved, "{}", path.display());
            // Working files left in the template directory would be embedded.
            let name = path.to_string_lossy();
            let stray = [
                ".terraform/",
                ".tfstate",
                ".tfvars.json",
                ".tfplan",
                "node_modules/",
            ];
            assert!(!stray.iter().any(|part| name.contains(part)), "{name}");
        }
        let catalog = fs::read_to_string(output.join("app/app-config.yaml"))?;
        assert!(catalog.contains("portal"));
        for required in [
            ".gitignore",
            ".github/workflows/preview.yml",
            "infra/env/.terraform.lock.hcl",
            "app/yarn.lock",
        ] {
            assert!(output.join(required).is_file(), "{required}");
        }
        Ok(())
    }

    #[test]
    fn init_never_writes_into_an_existing_directory() -> TestResult {
        let temp = tempfile::tempdir()?;
        let settings = Settings::sample(temp.path().to_path_buf()).validate()?;
        let refused = render_template(&settings, temp.path()).map_err(|error| error.code());
        assert_eq!(refused.err(), Some("INVALID_INPUT"));
        assert_eq!(files_under(temp.path())?.len(), 0);
        Ok(())
    }

    #[test]
    fn an_unknown_placeholder_is_an_error_not_silent_output() -> TestResult {
        let settings = Settings::sample(PathBuf::from(".")).validate()?;
        let unknown = render_contents(&settings, b"name: __BACKSTAGE_AWS_UNKNOWN__");
        assert!(unknown.is_err());
        let binary = render_contents(&settings, &[0xff, 0xfe, 0x00])?;
        assert_eq!(binary, vec![0xff, 0xfe, 0x00]);
        Ok(())
    }
}
