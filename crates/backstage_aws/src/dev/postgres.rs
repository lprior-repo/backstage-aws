use super::{COMMAND_LIMIT, STARTUP_LIMIT};
use crate::{
    container::ContainerEngine,
    error::{Error, Result},
    process,
};
use std::{
    collections::hash_map::DefaultHasher,
    ffi::OsStr,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::Duration,
};

/// Owns exactly one Compose project, never the host's other containers.
pub(super) struct Postgres {
    app: PathBuf,
    project: String,
    port: String,
    engine: ContainerEngine,
    cleanup_required: bool,
}

impl Postgres {
    pub(super) fn new(app: &Path, engine: ContainerEngine, port: u16) -> Self {
        Self {
            app: app.to_path_buf(),
            project: project_name(app),
            engine,
            port: port.to_string(),
            cleanup_required: false,
        }
    }

    pub(super) fn project(&self) -> &str {
        &self.project
    }

    pub(super) async fn verify_runtime(app: &Path, engine: ContainerEngine) -> Result<()> {
        if !app.join("compose.yaml").is_file() {
            return Err(Error::precondition_failed(
                "app/compose.yaml is missing; regenerate the project from the current template",
            ));
        }
        process::run(
            OsStr::new(engine.program()),
            &["info"],
            app,
            None,
            COMMAND_LIMIT,
        )
        .await
        .map_err(|error| {
            Error::precondition_failed(format!(
                "{engine} is unavailable: {error}; check `{engine} info` and your socket permissions"
            ))
        })?;
        process::run(
            OsStr::new(engine.program()),
            &["compose", "version"],
            app,
            None,
            COMMAND_LIMIT,
        )
        .await
        .map_err(|error| {
            Error::precondition_failed(format!("{engine} compose is required: {error}"))
        })
    }

    pub(super) async fn prepare(&mut self, reset: bool, created: &mut Vec<String>) -> Result<()> {
        self.cleanup_required = true;
        if reset {
            self.compose(&["down", "--volumes", "--remove-orphans"], COMMAND_LIMIT)
                .await?;
        }
        self.compose(
            &[
                "up",
                "--detach",
                "--wait",
                "--wait-timeout",
                &STARTUP_LIMIT.as_secs().to_string(),
                "postgres",
            ],
            Duration::from_mins(10),
        )
        .await?;
        created.push(format!("{}-compose:{}", self.engine, self.project));
        Ok(())
    }

    pub(super) async fn shutdown(&self) -> Result<()> {
        if !self.cleanup_required {
            return Ok(());
        }
        self.compose(&["down", "--remove-orphans"], COMMAND_LIMIT)
            .await
    }

    async fn compose(&self, arguments: &[&str], limit: Duration) -> Result<()> {
        let arguments: Vec<&str> = [
            "compose",
            "--file",
            "compose.yaml",
            "--project-name",
            &self.project,
        ]
        .into_iter()
        .chain(arguments.iter().copied())
        .collect();
        process::run_with_env(
            OsStr::new(self.engine.program()),
            &arguments,
            &self.app,
            None,
            limit,
            &[("BACKSTAGE_DEV_POSTGRES_PORT", &self.port)],
        )
        .await
        .map_err(|error| Error::tool_failed(format!("{} compose", self.engine), error.to_string()))
    }
}

fn project_name(app: &Path) -> String {
    let mut hash = DefaultHasher::new();
    app.hash(&mut hash);
    format!("backstage-dev-{:016x}", hash.finish())
}

#[cfg(test)]
mod tests {
    use super::project_name;
    use std::path::Path;

    #[test]
    fn compose_project_is_repeatable_and_project_local() {
        let first = project_name(Path::new("/projects/one/app"));
        assert_eq!(first, project_name(Path::new("/projects/one/app")));
        assert_ne!(first, project_name(Path::new("/projects/two/app")));
        assert!(first.starts_with("backstage-dev-"));
    }
}
