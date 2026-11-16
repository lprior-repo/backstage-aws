//! CLI contract scenarios for unavailable external container engines.
//! These stand-ins prove orchestration/cleanup, not live PostgreSQL or Backstage.
#![cfg(unix)]
use serde_json::Value;
use std::{
    error::Error,
    fs,
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Command, Output},
};

const BINARY: &str = env!("CARGO_BIN_EXE_backstage-aws");
type TestResult = Result<(), Box<dyn Error>>;

struct Project {
    directory: tempfile::TempDir,
    app: PathBuf,
    bin: PathBuf,
    log: PathBuf,
}

impl Project {
    fn new() -> Result<Self, Box<dyn Error>> {
        let directory = tempfile::tempdir()?;
        let app = directory.path().join("app");
        let bin = directory.path().join("bin");
        let log = directory.path().join("commands");
        fs::create_dir_all(app.join(".yarn/releases"))?;
        fs::create_dir_all(app.join("node_modules"))?;
        fs::create_dir_all(&bin)?;
        fs::write(
            app.join("package.json"),
            r#"{"engines":{"node":"24.22.1"},"packageManager":"yarn@4.13.0"}"#,
        )?;
        fs::write(app.join(".yarn/releases/yarn-4.13.0.cjs"), "")?;
        fs::write(app.join("compose.yaml"), "services: {}\n")?;
        fs::write(
            app.join("app-config.local.example.yaml"),
            "backend:\n  database:\n    connection:\n      port: 54320\n",
        )?;
        let project = Self {
            directory,
            app,
            bin,
            log,
        };
        project.executable("node", "#!/bin/sh\nif [ \"$1\" = --version ]; then echo v24.22.1; exit 0; fi\nprintf 'node %s\\n' \"$*\" >> \"$DEV_LOG\"\necho child-output\nif [ -n \"$DEV_NODE_EXIT\" ]; then exit \"$DEV_NODE_EXIT\"; fi\n")?;
        project.executable("docker", "#!/bin/sh\nprintf '%s %s port=%s\\n' \"${0##*/}\" \"$*\" \"$BACKSTAGE_DEV_POSTGRES_PORT\" >> \"$DEV_LOG\"\nif [ \"$1\" = info ] && [ \"${DEV_INFO_FAIL:-0}\" = 1 ]; then echo 'permission denied on container socket' >&2; exit 1; fi\ncase \"$*\" in *' up '*) exit \"${DEV_UP_EXIT:-0}\" ;; esac\necho container-output\n")?;
        fs::copy(project.bin.join("docker"), project.bin.join("podman"))?;
        Ok(project)
    }

    fn executable(&self, name: &str, source: &str) -> TestResult {
        let path = self.bin.join(name);
        fs::write(&path, source)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
        Ok(())
    }

    fn command(&self) -> Command {
        let mut command = Command::new(BINARY);
        command
            .env_clear()
            .env("PATH", &self.bin)
            .env("DEV_LOG", &self.log)
            .arg("--project")
            .arg(self.directory.path())
            .arg("dev");
        command
    }

    fn commands(&self) -> Result<String, Box<dyn Error>> {
        Ok(fs::read_to_string(&self.log)?)
    }
}

fn document(output: &Output) -> Result<Value, Box<dyn Error>> {
    Ok(serde_json::from_slice(&output.stdout)?)
}

#[test]
fn setup_only_waits_for_postgres_and_removes_containers_without_starting_app() -> TestResult {
    // Given a local project with dependencies and an available Docker boundary.
    let project = Project::new()?;
    // When setup-only is run on a nondefault database port.
    let output = project
        .command()
        .args(["--setup-only", "--port", "54399"])
        .output()?;
    // Then stdout is exactly one success JSON, the config matches, and cleanup ran.
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json = document(&output)?;
    assert_eq!(json.pointer("/ok"), Some(&Value::Bool(true)));
    assert_eq!(
        json.pointer("/result/postgres/port"),
        Some(&Value::from(54399))
    );
    let commands = project.commands()?;
    assert!(
        commands.contains("up --detach --wait --wait-timeout 90 postgres port=54399"),
        "{commands}"
    );
    assert!(
        commands.contains("down --remove-orphans port=54399"),
        "{commands}"
    );
    assert!(!commands.contains("node "), "{commands}");
    assert!(fs::read_to_string(project.app.join("app-config.local.yaml"))?.contains("port: 54399"));
    Ok(())
}

#[test]
fn docker_unavailable_is_blocked_before_config_or_container_changes() -> TestResult {
    // Given Docker socket permission is denied.
    let project = Project::new()?;
    // When dev is requested.
    let output = project.command().env("DEV_INFO_FAIL", "1").output()?;
    // Then there is one blocked JSON and no Compose mutation or config write.
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        document(&output)?.pointer("/error/code"),
        Some(&Value::from("PRECONDITION_FAILED"))
    );
    assert!(String::from_utf8_lossy(&output.stderr).contains("permission denied"));
    assert_eq!(project.commands()?, "docker info port=\n");
    assert!(!project.app.join("app-config.local.yaml").exists());
    Ok(())
}

#[test]
fn startup_failure_still_removes_partial_compose_resources() -> TestResult {
    // Given Compose fails while starting PostgreSQL.
    let project = Project::new()?;
    // When dev prepares its database.
    let output = project
        .command()
        .env("DEV_UP_EXIT", "1")
        .arg("--setup-only")
        .output()?;
    // Then it reports failure and attempts down without destroying the volume.
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(document(&output)?.pointer("/ok"), Some(&Value::Bool(false)));
    let commands = project.commands()?;
    assert!(commands.contains(" up --detach --wait "), "{commands}");
    assert!(commands.contains(" down --remove-orphans "), "{commands}");
    assert!(!commands.contains("--volumes"), "{commands}");
    Ok(())
}

#[test]
fn reset_deletes_volume_before_start_and_app_failure_still_cleans_up() -> TestResult {
    // Given an existing database and an app process which exits nonzero.
    let project = Project::new()?;
    // When reset and dev are requested.
    let output = project
        .command()
        .env("DEV_NODE_EXIT", "7")
        .arg("--reset")
        .output()?;
    // Then reset precedes startup, app output stays on stderr, and containers stop.
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(document(&output)?.pointer("/ok"), Some(&Value::Bool(false)));
    assert!(String::from_utf8_lossy(&output.stderr).contains("child-output"));
    let commands = project.commands()?;
    let reset = commands
        .find(" down --volumes --remove-orphans ")
        .ok_or("missing reset")?;
    let start = commands
        .find(" up --detach --wait ")
        .ok_or("missing start")?;
    let app = commands.find("node ").ok_or("missing app")?;
    let stop = commands
        .rfind(" down --remove-orphans ")
        .ok_or("missing stop")?;
    assert!(reset < start && start < app && app < stop, "{commands}");
    Ok(())
}

#[test]
fn no_install_requires_dependencies_and_mismatched_existing_port_is_preserved() -> TestResult {
    // Given missing dependencies, no-install must fail without starting Postgres.
    let project = Project::new()?;
    fs::remove_dir(project.app.join("node_modules"))?;
    let output = project.command().arg("--no-install").output()?;
    assert_eq!(output.status.code(), Some(3));
    assert!(!project.commands()?.contains(" up "));
    assert!(!project.commands()?.contains(" down "));
    // Given an existing config, a different port must fail without overwriting it.
    fs::create_dir(project.app.join("node_modules"))?;
    let config = project.app.join("app-config.local.yaml");
    let source = "backend:\n  database:\n    connection:\n      port: 54321\n";
    fs::write(&config, source)?;
    let output = project.command().arg("--setup-only").output()?;
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        document(&output)?.pointer("/error/code"),
        Some(&Value::from("PRECONDITION_FAILED"))
    );
    assert_eq!(fs::read_to_string(config)?, source);
    assert!(!project.commands()?.contains(" up "));
    assert!(!project.commands()?.contains(" down "));
    Ok(())
}

#[test]
fn ctrl_c_stops_the_owned_app_and_removes_containers_without_deleting_data() -> TestResult {
    // Given a running app process and the Docker orchestration boundary.
    use nix::{
        sys::signal::{Signal, kill},
        unistd::Pid,
    };
    use std::{process::Stdio, thread, time::Duration};
    let project = Project::new()?;
    project.executable(
        "node",
        "#!/bin/sh\nif [ \"$1\" = --version ]; then echo v24.22.1; exit 0; fi\nprintf '%s' \"$$\" > \"$DEV_LOG.pid\"\nprintf 'node %s\\n' \"$*\" >> \"$DEV_LOG\"\nexec /bin/sleep 30\n",
    )?;
    let mut child = project
        .command()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut started = false;
    for _ in 0..200 {
        if project
            .commands()
            .is_ok_and(|commands| commands.contains("node "))
        {
            started = true;
            break;
        }
        if child.try_wait()?.is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    if !started {
        child.kill()?;
        child.wait()?;
        return Err("dev did not start its app process".into());
    }
    // When the user interrupts dev, its separate process group must be reaped.
    kill(Pid::from_raw(i32::try_from(child.id())?), Signal::SIGINT)?;
    let output = child.wait_with_output()?;
    // Then it exits cleanly with one JSON and preserves the database volume.
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(document(&output)?.pointer("/ok"), Some(&Value::Bool(true)));
    let commands = project.commands()?;
    assert!(commands.contains(" down --remove-orphans "), "{commands}");
    assert!(!commands.contains("--volumes"), "{commands}");
    let process_id = fs::read_to_string(project.log.with_extension("pid"))?.parse::<i32>()?;
    assert_eq!(
        kill(Pid::from_raw(process_id), None),
        Err(nix::errno::Errno::ESRCH)
    );
    Ok(())
}

#[test]
fn podman_selection_uses_environment_and_explicit_flag_wins() -> TestResult {
    // Given either the environment selection or an explicit Docker override.
    for (flag, expected) in [(None, "podman"), (Some("docker"), "docker")] {
        let project = Project::new()?;
        let mut command = project.command();
        command
            .env("BACKSTAGE_AWS_CONTAINER_ENGINE", "podman")
            .arg("--setup-only");
        if let Some(engine) = flag {
            command.args(["--container-engine", engine]);
        }
        // When dev runs, it must use exactly that engine for every command.
        let output = command.output()?;
        assert!(output.status.success(), "{:?}", output.stderr);
        // Then the response names the engine and the other engine is untouched.
        assert_eq!(
            document(&output)?.pointer("/result/postgres/engine"),
            Some(&Value::from(expected))
        );
        let commands = project.commands()?;
        assert!(commands.lines().all(|line| line.starts_with(expected)));
        assert!(commands.contains(" down --remove-orphans "));
    }
    Ok(())
}

#[test]
fn unknown_container_engine_is_rejected_before_external_commands() -> TestResult {
    // Given a valid project and an unsupported container engine.
    let project = Project::new()?;
    // When the global flag is parsed.
    let output = project
        .command()
        .args(["--container-engine", "unknown"])
        .output()?;
    // Then one invalid-input JSON is emitted without running any child.
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        document(&output)?.pointer("/error/code"),
        Some(&Value::from("INVALID_INPUT"))
    );
    assert!(!project.log.exists());
    Ok(())
}
