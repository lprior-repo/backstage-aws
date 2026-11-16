//! A disposable, localhost-only rehearsal of PostgreSQL backup and Backstage runtime behavior.
use crate::{
    error::{Error, Result},
    process,
};
mod seed_recovery;
use reqwest::{Client, StatusCode};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    ffi::OsStr,
    fs,
    io::Write,
    net::TcpListener,
    path::{Path, PathBuf},
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::{
    process::Command,
    time::{Instant, sleep},
};

const NODE_VERSION: &str = "v24.22.1";
const POSTGRES_VERSION: &str = "16";
const POSTGRES_PASSWORD: &str = "local-runtime-postgres-password-sentinel";
const INITIAL_APP_PASSWORD: &str = "local-runtime-initial-app-password-sentinel";
const APP_ROLE: &str = "backstage_app";
const ROTATED_APP_PASSWORD: &str = "LocalRuntimeRotatedPasswordSentinel00000";
const ADMIN_PASSWORD: &str = "local-runtime-admin-password-sentinel";
const OPERATION_ID: &str = "11111111-1111-4111-8111-111111111111";
const FAILURE_OPERATION_ID: &str = "22222222-2222-4222-8222-222222222222";
const COMMAND_LIMIT: Duration = Duration::from_secs(60);
const READINESS_LIMIT: Duration = Duration::from_secs(90);
const EXPECTED_RESTORE: &str = "clone|alpha|changed\nclone|beta|two\nclone|gamma|three\n";

#[derive(Clone)]
pub struct Tools {
    pub(crate) postgres_bin: PathBuf,
    pub(crate) node: PathBuf,
}

struct Cluster {
    root: PathBuf,
    socket: PathBuf,
    log: PathBuf,
    process: process::OwnedChild,
}

impl Cluster {
    async fn shutdown(&mut self) -> Result<()> {
        self.process
            .shutdown()
            .await
            .map_err(|error| failure("local runtime", error.to_string()))
    }
}

struct Backend {
    process: process::OwnedChild,
    port: u16,
}
impl Backend {
    async fn shutdown(&mut self) -> Result<()> {
        self.process
            .shutdown()
            .await
            .map_err(|error| failure("local runtime", error.to_string()))
    }
}

pub async fn run(project: &Path, postgres_bin: Option<&Path>) -> Result<Vec<String>> {
    let tools = discover_tools(postgres_bin)?;
    verify_tool_versions(&tools).await?;
    let app = project.join("app");
    let archive = app.join("packages/backend/dist/bundle.tar.gz");
    if !archive.is_file() {
        return Err(failure(
            "local runtime",
            format!(
                "missing built backend bundle {} (run check --app first)",
                archive.display()
            ),
        ));
    }
    let temporary =
        tempfile::tempdir().map_err(|error| failure("local runtime", error.to_string()))?;
    let mut passed = vec!["local.runtime.tools".to_owned()];
    let mut cluster = start_cluster(&tools, temporary.path()).await?;
    rehearse_database(&tools, &cluster, project).await?;
    passed.extend([
        "local.postgres16.isolated".to_owned(),
        "local.postgres16.clone-dump-restore".to_owned(),
        "local.postgres16.seed-cutover-removes-stale-schema".to_owned(),
        "local.postgres16.restore-failure-preserves-seed".to_owned(),
        "local.postgres16.expired-seed-task-reconciliation".to_owned(),
        "local.postgres16.password-rotation".to_owned(),
        "local.postgres16.rotation-failure-does-not-disclose-secret".to_owned(),
    ]);
    let mut backend = start_backend(
        &tools,
        &app,
        temporary.path(),
        &cluster,
        ROTATED_APP_PASSWORD,
    )
    .await?;
    verify_backend(&mut backend).await?;
    passed.extend([
        "local.backstage.readiness".to_owned(),
        "local.backstage.guest-auth-permissions".to_owned(),
    ]);
    backend.shutdown().await?;
    cluster.shutdown().await?;
    Ok(passed)
}

pub fn discover_tools(postgres_bin: Option<&Path>) -> Result<Tools> {
    let node = find_on_path("node")
        .ok_or_else(|| failure("local runtime", "Node 24.22.1 was not found on PATH"))?
        .canonicalize()
        .map_err(|error| failure("local runtime", error.to_string()))?;
    let postgres_bin = match postgres_bin {
        Some(path) => path.to_path_buf(),
        None => find_on_path("postgres")
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .ok_or_else(|| {
                failure(
                    "local runtime",
                    "PostgreSQL 16 tools were not found on PATH",
                )
            })?,
    }
    .canonicalize()
    .map_err(|error| failure("local runtime", error.to_string()))?;
    let required = ["initdb", "postgres", "psql", "pg_dump", "pg_restore"];
    required.iter().try_for_each(|name| {
        let path = postgres_bin.join(name);
        if path.is_file() {
            Ok(())
        } else {
            Err(failure(
                "local runtime",
                format!("required PostgreSQL tool missing: {}", path.display()),
            ))
        }
    })?;
    Ok(Tools { postgres_bin, node })
}

pub fn find_on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
}

async fn verify_tool_versions(tools: &Tools) -> Result<()> {
    let node_version = output(&tools.node, &["--version"], Path::new(".")).await?;
    let node_version = String::from_utf8_lossy(&node_version).trim().to_owned();
    if node_version != NODE_VERSION {
        return Err(failure(
            "local runtime",
            format!("Node must be {NODE_VERSION}; found {node_version}"),
        ));
    }
    for tool in ["initdb", "postgres", "psql", "pg_dump", "pg_restore"] {
        let version = output(
            &tools.postgres_bin.join(tool),
            &["--version"],
            Path::new("."),
        )
        .await?;
        let version = String::from_utf8_lossy(&version);
        if !supported_postgres_version(&version) {
            return Err(failure(
                "local runtime",
                format!("{tool} must be PostgreSQL 16; found {}", version.trim()),
            ));
        }
    }
    Ok(())
}

pub fn supported_postgres_version(version: &str) -> bool {
    version
        .split_once(" (PostgreSQL) ")
        .and_then(|(_, version)| version.split_whitespace().next())
        .and_then(|version| version.split('.').next())
        == Some(POSTGRES_VERSION)
}

async fn output(program: &Path, args: &[&str], cwd: &Path) -> Result<Vec<u8>> {
    process::capture(program.as_os_str(), args, cwd, COMMAND_LIMIT).await
}

async fn start_cluster(tools: &Tools, root: &Path) -> Result<Cluster> {
    let data = root.join("postgres-data");
    let socket = root.join("postgres-socket");
    let password_file = root.join("postgres-password");
    fs::create_dir(&socket).map_err(|error| failure("local runtime", error.to_string()))?;
    fs::write(&password_file, format!("{POSTGRES_PASSWORD}\n"))
        .map_err(|error| failure("local runtime", error.to_string()))?;
    #[cfg(unix)]
    fs::set_permissions(&password_file, fs::Permissions::from_mode(0o600))
        .map_err(|error| failure("local runtime", error.to_string()))?;
    run_cmd(
        &tools.postgres_bin.join("initdb"),
        &[
            "-D",
            text(&data)?,
            "--pwfile",
            text(&password_file)?,
            "--auth-local=scram-sha-256",
            "--auth-host=reject",
            "--username=postgres",
            "--no-locale",
        ],
        root,
    )
    .await?;
    fs::remove_file(&password_file).map_err(|error| failure("local runtime", error.to_string()))?;
    let log = root.join("postgres.log");
    let log_file =
        fs::File::create(&log).map_err(|error| failure("local runtime", error.to_string()))?;
    let mut command = Command::new(tools.postgres_bin.join("postgres"));
    command
        .args([
            "-D",
            text(&data)?,
            "-k",
            text(&socket)?,
            "-p",
            "54321",
            "-c",
            "listen_addresses=",
            "-c",
            "unix_socket_permissions=0700",
        ])
        .env_clear()
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log_file));
    let process = process::spawn_owned(command)
        .map_err(|error| failure("local runtime", error.to_string()))?;
    let mut cluster = Cluster {
        root: root.to_path_buf(),
        socket,
        log,
        process,
    };
    ready_postgres(tools, &mut cluster).await?;
    Ok(cluster)
}

async fn ready_postgres(tools: &Tools, cluster: &mut Cluster) -> Result<()> {
    let deadline = Instant::now()
        .checked_add(READINESS_LIMIT)
        .ok_or_else(|| failure("local runtime", "PostgreSQL readiness deadline overflow"))?;
    loop {
        if Instant::now() >= deadline {
            return Err(failure("local runtime", "PostgreSQL readiness timed out"));
        }
        match cluster.process.try_wait() {
            Ok(None) => {}
            Ok(Some(status)) => {
                return Err(failure(
                    "local runtime",
                    format!("PostgreSQL exited before readiness: {status}"),
                ));
            }
            Err(error) => return Err(failure("local runtime", error.to_string())),
        }
        let args = [
            "-h",
            text(&cluster.socket)?,
            "-p",
            "54321",
            "-U",
            "postgres",
            "-d",
            "postgres",
            "-At",
            "-c",
            "SELECT 1",
        ];
        let psql = tools.postgres_bin.join("psql");
        let mut command = Command::new(psql);
        command
            .args(args)
            .current_dir(&cluster.root)
            .env_clear()
            .envs([
                ("PGPASSWORD", POSTGRES_PASSWORD),
                ("PGSSLMODE", "disable"),
                ("PSQLRC", "/dev/null"),
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::inherit());
        let (status, output) =
            process::capture_command(command, Duration::from_secs(5), 64).await?;
        if status.success() && output == b"1\n" {
            return Ok(());
        }
        sleep(Duration::from_millis(250)).await;
    }
}
async fn rehearse_database(tools: &Tools, cluster: &Cluster, project: &Path) -> Result<()> {
    rehearse_seed_initialization(tools, cluster, project).await?;
    let previous = rehearse_seed_restore(tools, cluster, project).await?;
    rehearse_failed_restore(tools, cluster, project, &previous).await?;
    rehearse_password_rotation(tools, cluster, project).await?;
    psql(
        tools,
        cluster,
        "postgres",
        "CREATE DATABASE backstage OWNER backstage_app;",
    )
    .await
}

async fn rehearse_seed_initialization(
    tools: &Tools,
    cluster: &Cluster,
    project: &Path,
) -> Result<()> {
    configure_database_admin(tools, cluster, ADMIN_PASSWORD).await?;
    let socket = text(&cluster.socket)?;
    let init_env = [
        ("PGHOST", socket),
        ("PGPORT", "54321"),
        ("PGDATABASE", "postgres"),
        ("PGUSER", "backstage_admin"),
        ("PGPASSWORD", ADMIN_PASSWORD),
        ("PGSSLMODE", "disable"),
        ("APP_ROLE", APP_ROLE),
        ("APP_PASSWORD", INITIAL_APP_PASSWORD),
        ("LIFECYCLE_ROLE", "backstage_lifecycle"),
        (
            "LIFECYCLE_PASSWORD",
            "local-runtime-lifecycle-password-sentinel",
        ),
    ];
    run_script(tools, project, "initialize-db.sh", &init_env).await?;
    app_password_login(tools, cluster, "seed", INITIAL_APP_PASSWORD).await?;
    psql(
        tools,
        cluster,
        "seed",
        "CREATE TABLE rehearsal_records (namespace text NOT NULL, record_key text NOT NULL, record_value text NOT NULL, PRIMARY KEY(namespace, record_key)); INSERT INTO rehearsal_records VALUES ('seed','alpha','one'),('seed','beta','two');",
    )
    .await?;
    psql(
        tools,
        cluster,
        "postgres",
        "CREATE DATABASE rehearsal_clone TEMPLATE seed;",
    )
    .await?;
    psql(
        tools,
        cluster,
        "rehearsal_clone",
        "UPDATE rehearsal_records SET namespace='clone', record_value=CASE WHEN record_key='alpha' THEN 'changed' ELSE record_value END; INSERT INTO rehearsal_records VALUES ('clone','gamma','three');",
    )
    .await?;
    psql(
        tools,
        cluster,
        "seed",
        "CREATE SCHEMA stale_schema; CREATE TABLE stale_schema.obsolete (value text);",
    )
    .await?;
    Ok(())
}

async fn rehearse_seed_restore(tools: &Tools, cluster: &Cluster, project: &Path) -> Result<String> {
    let socket = text(&cluster.socket)?;
    let dump = cluster.root.join("rehearsal.dump");
    let dump_args = [
        "-Fc",
        "-h",
        socket,
        "-p",
        "54321",
        "-U",
        "backstage_admin",
        "-d",
        "rehearsal_clone",
        "-f",
        text(&dump)?,
    ];
    let pg_dump = tools.postgres_bin.join("pg_dump");
    process::run_with_env(
        pg_dump.as_os_str(),
        &dump_args,
        cluster.root.as_path(),
        None,
        COMMAND_LIMIT,
        &[("PGPASSWORD", ADMIN_PASSWORD), ("PGSSLMODE", "disable")],
    )
    .await?;
    let dump_path = text(&dump)?;
    let object_key = format!("operations/seed/{OPERATION_ID}.dump");
    let restore_env = [
        ("PGHOST", socket),
        ("PGPORT", "54321"),
        ("PGDATABASE", "postgres"),
        ("PGUSER", "backstage_admin"),
        ("PGPASSWORD", ADMIN_PASSWORD),
        ("PGSSLMODE", "disable"),
        ("APP_ROLE", APP_ROLE),
        ("SEED_OPERATION_ID", OPERATION_ID),
        ("SEED_OBJECT_KEY", object_key.as_str()),
        ("DUMP_PATH", dump_path),
    ];
    run_script(tools, project, "restore-seed.sh", &restore_env).await?;
    validate_restored_records(&query(tools, cluster, "seed").await?)?;
    let previous = format!("seed_previous_{}", OPERATION_ID.replace('-', ""));
    let previous_records = query(tools, cluster, &previous).await?;
    if previous_records != "seed|alpha|one\nseed|beta|two\n" {
        return Err(failure(
            "local runtime",
            format!("clone mutation changed previous seed: {previous_records:?}"),
        ));
    }
    let stale_schema = psql_query(
        tools,
        cluster,
        "seed",
        "SELECT EXISTS (SELECT FROM pg_namespace WHERE nspname = 'stale_schema')",
    )
    .await?;
    if stale_schema.trim() != "f" {
        return Err(failure(
            "local runtime",
            "seed restore retained a schema absent from the dump",
        ));
    }
    seed_recovery::rehearse(tools, cluster, project, &restore_env, &previous).await?;
    Ok(previous)
}

async fn rehearse_failed_restore(
    tools: &Tools,
    cluster: &Cluster,
    project: &Path,
    previous: &str,
) -> Result<()> {
    let socket = text(&cluster.socket)?;
    let invalid_dump = cluster.root.join("invalid.dump");
    fs::write(&invalid_dump, b"not a PostgreSQL custom-format dump")
        .map_err(|error| failure("local runtime", error.to_string()))?;
    let object_key = format!("operations/seed/{FAILURE_OPERATION_ID}.dump");
    let failure_restore_env = [
        ("PGHOST", socket),
        ("PGPORT", "54321"),
        ("PGDATABASE", "postgres"),
        ("PGUSER", "backstage_admin"),
        ("PGPASSWORD", ADMIN_PASSWORD),
        ("PGSSLMODE", "disable"),
        ("APP_ROLE", APP_ROLE),
        ("SEED_OPERATION_ID", FAILURE_OPERATION_ID),
        ("SEED_OBJECT_KEY", object_key.as_str()),
        ("DUMP_PATH", text(&invalid_dump)?),
    ];
    if run_script(tools, project, "restore-seed.sh", &failure_restore_env)
        .await
        .is_ok()
    {
        return Err(failure("local runtime", "restore accepted an invalid dump"));
    }
    let failed_stage = format!("seed_stage_{}", FAILURE_OPERATION_ID.replace('-', ""));
    let stage_query =
        format!("SELECT EXISTS (SELECT FROM pg_database WHERE datname = '{failed_stage}')");
    if psql_query(tools, cluster, "postgres", &stage_query)
        .await?
        .trim()
        != "t"
    {
        return Err(failure(
            "local runtime",
            "invalid dump failed before PostgreSQL restore reached its stage database",
        ));
    }
    validate_restored_records(&query(tools, cluster, "seed").await?)?;
    validate_restored_records(&query(tools, cluster, "rehearsal_clone").await?)?;
    let previous_after_failure = query(tools, cluster, previous).await?;
    if previous_after_failure != "seed|alpha|one\nseed|beta|two\n" {
        return Err(failure(
            "local runtime",
            format!("failed restore changed the previous seed: {previous_after_failure:?}"),
        ));
    }
    Ok(())
}

async fn rehearse_password_rotation(
    tools: &Tools,
    cluster: &Cluster,
    project: &Path,
) -> Result<()> {
    let socket = text(&cluster.socket)?;
    // Match the deployed script's file-only secret contract with a private synthetic fixture.
    let mut rotation_password = tempfile::NamedTempFile::new_in(&cluster.root)
        .map_err(|error| failure("local runtime", error.to_string()))?;
    rotation_password
        .write_all(ROTATED_APP_PASSWORD.as_bytes())
        .map_err(|error| failure("local runtime", error.to_string()))?;
    rotation_password
        .flush()
        .map_err(|error| failure("local runtime", error.to_string()))?;
    let rotation_password_path = text(rotation_password.path())?;
    let rotate_env = [
        ("PGHOST", socket),
        ("PGPORT", "54321"),
        ("PGDATABASE", "postgres"),
        ("PGUSER", "backstage_admin"),
        ("PGPASSWORD", ADMIN_PASSWORD),
        ("PGSSLMODE", "disable"),
        ("APP_ROLE", APP_ROLE),
        ("APP_PASSWORD_FILE", rotation_password_path),
    ];
    run_script(tools, project, "rotate-db.sh", &rotate_env).await?;
    app_password_login(tools, cluster, "seed", ROTATED_APP_PASSWORD).await?;
    if app_password_login(tools, cluster, "seed", INITIAL_APP_PASSWORD)
        .await
        .is_ok()
    {
        return Err(failure(
            "local runtime",
            "previous PostgreSQL password still authenticates after rotation",
        ));
    }
    rehearse_failed_rotation(tools, cluster, project, rotation_password_path).await?;
    app_password_login(tools, cluster, "seed", ROTATED_APP_PASSWORD).await?;
    rotation_password
        .close()
        .map_err(|error| failure("local runtime", error.to_string()))
}

async fn rehearse_failed_rotation(
    tools: &Tools,
    cluster: &Cluster,
    project: &Path,
    rotation_password_path: &str,
) -> Result<()> {
    let socket = text(&cluster.socket)?;
    let invalid_role_env = [
        ("PGHOST", socket),
        ("PGPORT", "54321"),
        ("PGDATABASE", "postgres"),
        ("PGUSER", "backstage_admin"),
        ("PGPASSWORD", ADMIN_PASSWORD),
        ("PGSSLMODE", "disable"),
        ("APP_ROLE", "backstage_missing_role"),
        ("APP_PASSWORD_FILE", rotation_password_path),
    ];
    let (status, output) =
        run_script_capture(tools, project, "rotate-db.sh", &invalid_role_env).await?;
    let output = String::from_utf8_lossy(&output);
    if status.success() || !output.contains("role \"backstage_missing_role\" does not exist") {
        return Err(failure(
            "local runtime",
            "rotation with a missing role did not fail at PostgreSQL role lookup",
        ));
    }
    if output.contains(ROTATED_APP_PASSWORD) {
        return Err(failure(
            "local runtime",
            "rotation child output disclosed its password sentinel",
        ));
    }
    let postgres_log =
        fs::read(&cluster.log).map_err(|error| failure("local runtime", error.to_string()))?;
    let postgres_log = String::from_utf8_lossy(&postgres_log);
    if !postgres_log.contains("role \"backstage_missing_role\" does not exist") {
        return Err(failure(
            "local runtime",
            "PostgreSQL did not log the induced rotation SQL failure",
        ));
    }
    if postgres_log.contains(ROTATED_APP_PASSWORD) {
        return Err(failure(
            "local runtime",
            "PostgreSQL logs disclosed the rotation password sentinel",
        ));
    }
    Ok(())
}

async fn run_script(tools: &Tools, project: &Path, name: &str, env: &[(&str, &str)]) -> Result<()> {
    let (status, _) = run_script_capture(tools, project, name, env).await?;
    if !status.success() {
        return Err(failure(name, format!("exited with {status}")));
    }
    Ok(())
}
async fn run_script_capture(
    tools: &Tools,
    project: &Path,
    name: &str,
    env: &[(&str, &str)],
) -> Result<(ExitStatus, Vec<u8>)> {
    let script = project.join("infra/base/scripts").join(name);
    if !script.is_file() {
        return Err(failure(
            "local runtime",
            format!("generated database script is missing: {}", script.display()),
        ));
    }
    let inherited_path = std::env::var_os("PATH").ok_or_else(|| {
        failure(
            "local runtime",
            "PATH is not set for generated database scripts",
        )
    })?;
    let path = std::env::join_paths(
        std::iter::once(tools.postgres_bin.clone()).chain(std::env::split_paths(&inherited_path)),
    )
    .map_err(|error| failure("local runtime", error.to_string()))?;
    let path = path
        .to_str()
        .ok_or_else(|| failure("local runtime", "script PATH is not valid UTF-8"))?;
    let environment = [
        ("PATH", path),
        ("APP_PASSWORD_FILE", ""),
        ("PSQLRC", "/dev/null"),
        ("AWS_EC2_METADATA_DISABLED", "true"),
        ("AWS_ACCESS_KEY_ID", ""),
        ("AWS_SECRET_ACCESS_KEY", ""),
        ("AWS_SESSION_TOKEN", ""),
        ("AWS_PROFILE", ""),
        ("AWS_SHARED_CREDENTIALS_FILE", ""),
    ]
    .into_iter()
    .chain(env.iter().copied())
    .collect::<Vec<_>>();
    let args = [
        "-c",
        "exec 2>&1; exec /bin/sh \"$1\"",
        "local-check",
        text(&script)?,
    ];
    let mut command = Command::new(OsStr::new("/bin/sh"));
    command
        .args(args)
        .current_dir(project)
        .env_clear()
        .envs(environment)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    process::capture_command(command, COMMAND_LIMIT, 64 * 1024).await
}

async fn configure_database_admin(tools: &Tools, cluster: &Cluster, password: &str) -> Result<()> {
    let args = [
        "-h",
        text(&cluster.socket)?,
        "-p",
        "54321",
        "-U",
        "postgres",
        "-d",
        "postgres",
        "-v",
        "ON_ERROR_STOP=1",
    ];
    let sql = b"\\getenv admin_password ADMIN_PASSWORD\nCREATE ROLE backstage_admin LOGIN SUPERUSER PASSWORD :'admin_password';\n";
    let psql = tools.postgres_bin.join("psql");
    process::run_with_env(
        psql.as_os_str(),
        &args,
        cluster.root.as_path(),
        Some(sql),
        COMMAND_LIMIT,
        &[
            ("PGPASSWORD", POSTGRES_PASSWORD),
            ("PGSSLMODE", "disable"),
            ("PSQLRC", "/dev/null"),
            ("ADMIN_PASSWORD", password),
        ],
    )
    .await
}

async fn psql(tools: &Tools, cluster: &Cluster, db: &str, sql: &str) -> Result<()> {
    let args = [
        "-h",
        text(&cluster.socket)?,
        "-p",
        "54321",
        "-U",
        "postgres",
        "-d",
        db,
        "-v",
        "ON_ERROR_STOP=1",
        "-c",
        sql,
    ];
    let psql = tools.postgres_bin.join("psql");
    process::run_with_env(
        psql.as_os_str(),
        &args,
        cluster.root.as_path(),
        None,
        COMMAND_LIMIT,
        &[
            ("PGPASSWORD", POSTGRES_PASSWORD),
            ("PGSSLMODE", "disable"),
            ("PSQLRC", "/dev/null"),
        ],
    )
    .await
}
async fn app_password_login(
    tools: &Tools,
    cluster: &Cluster,
    database: &str,
    password: &str,
) -> Result<()> {
    let args = [
        "-h",
        text(&cluster.socket)?,
        "-p",
        "54321",
        "-U",
        APP_ROLE,
        "-d",
        database,
        "-v",
        "ON_ERROR_STOP=1",
        "-c",
        "SELECT 1",
    ];
    let psql = tools.postgres_bin.join("psql");
    process::run_with_env(
        psql.as_os_str(),
        &args,
        cluster.root.as_path(),
        None,
        COMMAND_LIMIT,
        &[
            ("PGPASSWORD", password),
            ("PGSSLMODE", "disable"),
            ("PSQLRC", "/dev/null"),
        ],
    )
    .await
}

async fn query(tools: &Tools, cluster: &Cluster, db: &str) -> Result<String> {
    psql_query(
        tools,
        cluster,
        db,
        "SELECT namespace,record_key,record_value FROM rehearsal_records ORDER BY namespace,record_key",
    )
    .await
}

async fn psql_query(tools: &Tools, cluster: &Cluster, db: &str, sql: &str) -> Result<String> {
    let args = [
        "-h",
        text(&cluster.socket)?,
        "-p",
        "54321",
        "-U",
        "postgres",
        "-d",
        db,
        "-At",
        "-F",
        "|",
        "-v",
        "ON_ERROR_STOP=1",
        "-c",
        sql,
    ];
    let psql = tools.postgres_bin.join("psql");
    let mut command = Command::new(psql);
    command
        .args(args)
        .current_dir(&cluster.root)
        .env_clear()
        .envs([
            ("PGPASSWORD", POSTGRES_PASSWORD),
            ("PGSSLMODE", "disable"),
            ("PSQLRC", "/dev/null"),
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::inherit());
    let (status, output) = process::capture_command(command, COMMAND_LIMIT, 64 * 1024).await?;
    if !status.success() {
        return Err(failure(
            "local runtime",
            format!("psql query failed with {status}"),
        ));
    }
    String::from_utf8(output).map_err(|error| failure("local runtime", error.to_string()))
}
fn validate_restored_records(actual: &str) -> Result<()> {
    if actual == EXPECTED_RESTORE {
        Ok(())
    } else {
        Err(failure(
            "local runtime",
            format!("restored records did not match expected data: {actual:?}"),
        ))
    }
}

async fn start_backend(
    tools: &Tools,
    app: &Path,
    root: &Path,
    cluster: &Cluster,
    database_password: &str,
) -> Result<Backend> {
    let bundle = app.join("packages/backend/dist/bundle.tar.gz");
    let port = reserve_port()?;
    run_cmd(
        Path::new("tar"),
        &["-xzf", text(&bundle)?, "-C", text(root)?],
        app,
    )
    .await?;
    let modules = app.join("node_modules");
    if !modules.exists() {
        return Err(failure(
            "local runtime",
            "app/node_modules is missing; build/install the app first",
        ));
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(&modules, root.join("node_modules"))
        .map_err(|error| failure("local runtime", error.to_string()))?;
    let config = root.join("local-check.yaml");
    fs::write(&config, format!("app:\n  baseUrl: http://127.0.0.1:{port}\nbackend:\n  baseUrl: http://127.0.0.1:{port}\n  listen:\n    host: 127.0.0.1\n    port: {port}\n  cors:\n    origin: http://127.0.0.1:{port}\n  database:\n    client: pg\n    pluginDivisionMode: schema\n    connection:\n      host: {}\n      port: 54321\n      database: backstage\n      user: {APP_ROLE}\n      password: {database_password}\n      ssl: false\n    knexConfig:\n      pool:\n        min: 0\n        max: 2\nauth:\n  environment: production\n  session:\n    secret: local-rehearsal-only-session-secret\n  providers:\n    guest:\n      dangerouslyAllowOutsideDevelopment: true\ncatalog:\n  locations:\n    - type: file\n      target: ./catalog/org.yaml\n      rules:\n        - allow: [User, Group]\n    - type: file\n      target: ./catalog/entities.yaml\n", yaml_quote(&cluster.socket))).map_err(|error| failure("local runtime", error.to_string()))?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(app.join("catalog"), root.join("catalog"))
        .map_err(|error| failure("local runtime", error.to_string()))?;
    let path = std::env::var_os("PATH")
        .ok_or_else(|| failure("local runtime", "PATH is not set for Backstage"))?;
    let mut command = Command::new(&tools.node);
    command
        .args([
            "packages/backend",
            "--config",
            text(&app.join("app-config.yaml"))?,
            "--config",
            text(&app.join("app-config.auth-guest.yaml"))?,
            "--config",
            text(&config)?,
        ])
        .current_dir(root)
        .env_clear()
        .env("PATH", path)
        .env("HOME", root)
        .env("NODE_ENV", "production")
        .env("AWS_EC2_METADATA_DISABLED", "true")
        .stdin(Stdio::null())
        .stdout(Stdio::from(std::io::stderr()))
        .stderr(Stdio::inherit());
    let child = process::spawn_owned(command)
        .map_err(|error| failure("local runtime", error.to_string()))?;
    Ok(Backend {
        process: child,
        port,
    })
}

async fn verify_backend(backend: &mut Backend) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|error| failure("local runtime", error.to_string()))?;
    let readiness = format!(
        "http://127.0.0.1:{}/.backstage/health/v1/readiness",
        backend.port
    );
    let deadline = Instant::now()
        .checked_add(READINESS_LIMIT)
        .ok_or_else(|| failure("local runtime", "Backstage readiness deadline overflow"))?;
    loop {
        if Instant::now() >= deadline {
            return Err(failure("local runtime", "Backstage readiness timed out"));
        }
        match backend.process.try_wait() {
            Ok(None) => {}
            Ok(Some(status)) => {
                return Err(failure(
                    "local runtime",
                    format!("Backstage process exited before readiness: {status}"),
                ));
            }
            Err(error) => return Err(failure("local runtime", error.to_string())),
        }
        match client.get(&readiness).send().await {
            Ok(response) if response.status().is_success() => break,
            Ok(_) | Err(_) => sleep(Duration::from_millis(500)).await,
        }
    }
    guest_and_permissions(&client, backend.port).await
}

async fn guest_and_permissions(client: &Client, port: u16) -> Result<()> {
    let auth = client
        .post(format!("http://127.0.0.1:{port}/api/auth/guest/refresh"))
        .json(&serde_json::json!({}))
        .send()
        .await
        .map_err(|error| failure("local runtime", error.to_string()))?;
    if !auth.status().is_success() {
        return Err(failure(
            "local runtime",
            format!("guest auth failed with HTTP {}", auth.status()),
        ));
    }
    let payload: serde_json::Value = auth
        .json()
        .await
        .map_err(|error| failure("local runtime", error.to_string()))?;
    let token = payload
        .pointer("/backstageIdentity/token")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            failure(
                "local runtime",
                "guest response omitted backstageIdentity.token",
            )
        })?;
    let permission = client
        .post(format!("http://127.0.0.1:{port}/api/permission/authorize"))
        .bearer_auth(token)
        .json(&serde_json::json!({"items": [
            {"id": "read", "resourceRef": "component:default/backstage", "permission": {"type": "resource", "resourceType": "catalog-entity", "name": "catalog.entity.read", "attributes": {"action": "read"}}},
            {"id": "create", "permission": {"type": "basic", "name": "catalog.entity.create", "attributes": {"action": "create"}}}
        ]}))
        .send()
        .await
        .map_err(|error| failure("local runtime", error.to_string()))?;
    if permission.status() != StatusCode::OK {
        return Err(failure(
            "local runtime",
            format!(
                "permission authorization failed with HTTP {}",
                permission.status()
            ),
        ));
    }
    let result: serde_json::Value = permission
        .json()
        .await
        .map_err(|error| failure("local runtime", error.to_string()))?;
    let items = result
        .get("items")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| failure("local runtime", "permission response omitted items"))?;
    let decision = |id| {
        items
            .iter()
            .find(|item| item.get("id").and_then(serde_json::Value::as_str) == Some(id))
            .and_then(|item| item.get("result"))
            .and_then(serde_json::Value::as_str)
    };
    if items.len() != 2 || decision("read") != Some("ALLOW") || decision("create") != Some("DENY") {
        return Err(failure(
            "local runtime",
            format!("unexpected guest permission decisions: {items:?}"),
        ));
    }
    Ok(())
}

async fn run_cmd(program: &Path, args: &[&str], cwd: &Path) -> Result<()> {
    process::run(program.as_os_str(), args, cwd, None, COMMAND_LIMIT).await
}

fn reserve_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| failure("local runtime", error.to_string()))?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| failure("local runtime", error.to_string()))
}
fn text(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| failure("local runtime", "temporary path is not valid UTF-8"))
}
fn yaml_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "''"))
}
fn failure(tool: &str, detail: impl Into<String>) -> Error {
    Error::tool_failed(tool, detail)
}

#[cfg(test)]
mod tests {
    use super::{EXPECTED_RESTORE, supported_postgres_version, validate_restored_records};

    #[test]
    fn postgres_version_is_supported_only_when_major_is_sixteen() {
        assert!(supported_postgres_version("postgres (PostgreSQL) 16.4"));
        assert!(!supported_postgres_version("postgres (PostgreSQL) 18.0"));
        assert!(!supported_postgres_version("PostgreSQL 16.4"));
    }

    #[test]
    fn restore_records_are_accepted_only_when_exactly_expected() {
        assert!(validate_restored_records(EXPECTED_RESTORE).is_ok());
        assert!(validate_restored_records("clone|alpha|changed\n").is_err());
        assert!(
            validate_restored_records(
                "clone|alpha|changed\nclone|beta|two\nclone|gamma|three\nextra|x|y\n"
            )
            .is_err()
        );
        assert!(validate_restored_records("seed|alpha|one\nseed|beta|two\n").is_err());
    }
}
