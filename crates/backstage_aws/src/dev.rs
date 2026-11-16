//! Offline local development for a project-local Backstage application.
use crate::{
    container::ContainerEngine,
    error::{Error, Result},
    local_check, process,
};
use serde::Serialize;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::process::Command;

mod postgres;
mod setup;
use postgres::Postgres;

const URL: &str = "http://localhost:3000";
const LOCAL_CONFIG: &str = "app-config.local.yaml";
const EXAMPLE_CONFIG: &str = "app-config.local.example.yaml";
const STARTUP_LIMIT: Duration = Duration::from_secs(90);
const COMMAND_LIMIT: Duration = Duration::from_secs(90);
const INSTALL_LIMIT: Duration = Duration::from_mins(30);

/// Start Backstage locally using PostgreSQL 16 with the selected Compose engine.
#[derive(clap::Args, Clone, Debug)]
pub struct DevArgs {
    /// PostgreSQL TCP port on 127.0.0.1.
    #[arg(long, default_value_t = 54320)]
    port: u16,
    /// Skip dependency installation; existing `app/node_modules` is required.
    #[arg(long)]
    no_install: bool,
    /// Prepare the database and config, install dependencies, then exit without starting Yarn.
    #[arg(long)]
    setup_only: bool,
    /// Delete the command-managed Compose volume before preparing a fresh database.
    #[arg(long)]
    reset: bool,
}

#[derive(Serialize)]
struct DevResponse {
    url: &'static str,
    app: String,
    postgres: PostgresResponse,
    config: &'static str,
    created: Vec<String>,
}

#[derive(Serialize)]
struct PostgresResponse {
    port: u16,
    engine: ContainerEngine,
    project: String,
}

/// Run the project-local PostgreSQL and Backstage development workflow.
pub async fn run(project: &Path, engine: ContainerEngine, args: &DevArgs) -> Result<Value> {
    let app = validate_runtime(project, engine, args).await?;
    let mut created = Vec::new();
    let mut postgres = Postgres::new(&app, engine, args.port);
    run_session(&app, args, &mut postgres, &mut created).await?;
    serde_json::to_value(DevResponse {
        url: URL,
        app: app.display().to_string(),
        postgres: PostgresResponse {
            port: args.port,
            engine,
            project: postgres.project().to_owned(),
        },
        config: LOCAL_CONFIG,
        created,
    })
    .map_err(|error| Error::state_failed(error.to_string()))
}

async fn validate_runtime(
    project: &Path,
    engine: ContainerEngine,
    args: &DevArgs,
) -> Result<PathBuf> {
    if args.port == 0 {
        return Err(Error::invalid_input("--port must be between 1 and 65535"));
    }
    let app = project.join("app");
    let package_path = app.join("package.json");
    if !package_path.is_file() {
        return Err(Error::precondition_failed(format!(
            "{} is missing; run `backstage-aws init` to create the app",
            package_path.display()
        )));
    }
    let app = app
        .canonicalize()
        .map_err(|error| failure(format!("cannot resolve {}: {error}", app.display())))?;
    let package = read_text(&app.join("package.json"))?;
    let expected_node = setup::required_node_version(&app, &package)?;
    let node = local_check::find_on_path("node").ok_or_else(|| {
        Error::precondition_failed(format!(
            "Node {expected_node} is required but was not found on PATH"
        ))
    })?;
    let actual_node = setup::read_node_version(&node, &app).await?;
    if actual_node != expected_node {
        return Err(Error::precondition_failed(format!(
            "Node {expected_node} is required by this app; PATH provides {actual_node}"
        )));
    }
    setup::yarn_release(&app)?;
    Postgres::verify_runtime(&app, engine).await?;
    Ok(app)
}

async fn run_session(
    app: &Path,
    args: &DevArgs,
    postgres: &mut Postgres,
    created: &mut Vec<String>,
) -> Result<()> {
    let setup_result = prepare_with_interrupt(postgres, app, args, created).await;
    let session_result = match setup_result {
        Ok(()) if args.setup_only => Ok(()),
        Ok(()) => run_backstage(app).await,
        Err(error) => Err(error),
    };
    finish_session(session_result, postgres).await
}

async fn prepare_with_interrupt(
    postgres: &mut Postgres,
    app: &Path,
    args: &DevArgs,
    created: &mut Vec<String>,
) -> Result<()> {
    let mut interrupt = Box::pin(tokio::signal::ctrl_c());
    tokio::select! {
        biased;
        result = setup::prepare(postgres, app, args, created) => result,
        signal = &mut interrupt => match signal {
            Ok(()) => Err(failure("interrupted before local development setup completed")),
            Err(error) => Err(failure(format!("cannot listen for Ctrl-C: {error}"))),
        },
    }
}

async fn finish_session(session_result: Result<()>, postgres: &Postgres) -> Result<()> {
    let shutdown_result = postgres.shutdown().await;
    match (session_result, shutdown_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
        (Err(error), Err(shutdown)) => Err(failure(format!(
            "{error}; PostgreSQL shutdown failed: {shutdown}"
        ))),
    }
}

async fn run_backstage(app: &Path) -> Result<()> {
    let mut stderr = std::io::stderr();
    std::io::Write::write_all(
        &mut stderr,
        b"Backstage: http://localhost:3000 (Ctrl-C to stop)\n",
    )
    .map_err(|error| failure(format!("cannot write startup notice: {error}")))?;

    let yarn = setup::yarn_release(app)?;
    let mut command = Command::new("node");
    command
        .arg(yarn)
        .arg("start")
        .current_dir(app)
        .stdin(Stdio::inherit())
        .stdout(Stdio::from(std::io::stderr()))
        .stderr(Stdio::inherit());
    let mut child = process::spawn_owned(command)
        .map_err(|error| Error::tool_failed("yarn start", error.to_string()))?;
    let mut interrupt = Box::pin(tokio::signal::ctrl_c());
    tokio::select! {
        status = child.wait() => {
            let status = status.map_err(|error| Error::tool_failed("yarn start", error.to_string()))?;
            process_exit("yarn start", status)
        }
        signal = &mut interrupt => {
            signal.map_err(|error| failure(format!("cannot listen for Ctrl-C: {error}")))?;
            child
                .shutdown()
                .await
                .map_err(|error| Error::tool_failed("yarn start shutdown", error.to_string()))?;
            Ok(())
        }
    }
}

fn process_exit(name: &str, status: ExitStatus) -> Result<()> {
    status
        .success()
        .then_some(())
        .ok_or_else(|| Error::tool_failed(name, format!("exited with {status}")))
}

fn read_text(path: &Path) -> Result<String> {
    fs::read_to_string(path)
        .map_err(|error| failure(format!("cannot read {}: {error}", path.display())))
}

fn failure(detail: impl Into<String>) -> Error {
    Error::tool_failed("local dev", detail)
}
