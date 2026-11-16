use super::{
    COMMAND_LIMIT, DevArgs, EXAMPLE_CONFIG, INSTALL_LIMIT, LOCAL_CONFIG, failure,
    postgres::Postgres, read_text,
};
use crate::{
    app_config,
    error::{Error, Result},
    process,
};
use serde_json::Value;
use std::{
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
fn create_local_config(app: &Path, port: u16) -> Result<bool> {
    let local = app.join(LOCAL_CONFIG);
    if local.exists() {
        let configured = app_config::get(app, LOCAL_CONFIG, "backend.database.connection.port")?;
        if configured.get("value").and_then(Value::as_u64) != Some(u64::from(port)) {
            return Err(Error::precondition_failed(format!(
                "{LOCAL_CONFIG} backend.database.connection.port does not match --port {port}; change the config or use the matching --port"
            )));
        }
        return Ok(false);
    }
    let example = app.join(EXAMPLE_CONFIG);
    let source = read_text(&example)?;
    let example_port = app_config::get(app, EXAMPLE_CONFIG, "backend.database.connection.port")?;
    let example_port = example_port
        .get("value")
        .and_then(Value::as_u64)
        .ok_or_else(|| {
            Error::state_failed(format!(
                "{} has no integer backend.database.connection.port",
                example.display()
            ))
        })?;
    let content = if example_port == u64::from(port) {
        source
    } else {
        app_config::set_text(
            &source,
            "backend.database.connection.port",
            &port.to_string(),
        )?
    };
    write_new_file(&local, content.as_bytes())
}

fn write_new_file(path: &Path, content: &[u8]) -> Result<bool> {
    let mut file = match OpenOptions::new().write(true).create_new(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(false),
        Err(error) => {
            return Err(failure(format!(
                "cannot create {}: {error}",
                path.display()
            )));
        }
    };
    if let Err(error) = file.write_all(content) {
        let cleanup = fs::remove_file(path);
        return match cleanup {
            Ok(()) => Err(failure(format!("cannot write {}: {error}", path.display()))),
            Err(cleanup_error) => Err(failure(format!(
                "cannot write {}: {error}; cannot remove partial file: {cleanup_error}",
                path.display()
            ))),
        };
    }
    Ok(true)
}

pub(super) fn required_node_version(app: &Path, package: &str) -> Result<String> {
    let nvmrc = app.join(".nvmrc");
    if nvmrc.is_file() {
        return exact_node_pin(&read_text(&nvmrc)?).ok_or_else(|| {
            Error::precondition_failed(format!(
                "{} must contain an exact Node version such as 24.22.1",
                nvmrc.display()
            ))
        });
    }
    let versions_file = app.join("provenance/versions.json");
    if versions_file.is_file() {
        let contents = read_text(&versions_file)?;
        let versions: Value = serde_json::from_str(&contents)
            .map_err(|error| failure(format!("invalid {}: {error}", versions_file.display())))?;
        if let Some(version) = versions.get("nodeVersion").and_then(Value::as_str) {
            return exact_node_pin(version).ok_or_else(|| {
                Error::precondition_failed(format!(
                    "{} must declare an exact Node version",
                    versions_file.display()
                ))
            });
        }
    }
    let package: Value = serde_json::from_str(package)
        .map_err(|error| failure(format!("invalid app/package.json: {error}")))?;
    let engine = package
        .get("engines")
        .and_then(|engines| engines.get("node"))
        .and_then(Value::as_str)
        .and_then(exact_node_pin);
    engine.ok_or_else(|| {
        Error::precondition_failed(
            "could not determine an exact Node version; set app/.nvmrc or package.json engines.node to a full version",
        )
    })
}

fn exact_node_pin(value: &str) -> Option<String> {
    let value = value.trim();
    let version = value.strip_prefix('v').map_or(value, |version| version);
    if version.split('.').count() != 3
        || version
            .split('.')
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    Some(format!("v{version}"))
}

pub(super) async fn read_node_version(node: &Path, app: &Path) -> Result<String> {
    let output = process::capture(node.as_os_str(), &["--version"], app, COMMAND_LIMIT)
        .await
        .map_err(|error| Error::tool_failed("node --version", error.to_string()))?;
    Ok(String::from_utf8_lossy(&output).trim().to_owned())
}

pub(super) fn yarn_release(app: &Path) -> Result<PathBuf> {
    let package: Value = serde_json::from_str(&read_text(&app.join("package.json"))?)
        .map_err(|error| failure(format!("invalid app/package.json: {error}")))?;
    let version = package
        .get("packageManager")
        .and_then(Value::as_str)
        .and_then(|manager| manager.strip_prefix("yarn@"))
        .filter(|version| exact_node_pin(version).is_some())
        .ok_or_else(|| {
            Error::precondition_failed("app/package.json must pin an exact yarn@VERSION")
        })?;
    let yarn = app.join(format!(".yarn/releases/yarn-{version}.cjs"));
    if !yarn.is_file() {
        return Err(Error::precondition_failed(format!(
            "bundled Yarn is missing: {}",
            yarn.display()
        )));
    }
    Ok(yarn)
}
async fn install_dependencies(app: &Path) -> Result<()> {
    let yarn = yarn_release(app)?;
    process::run(
        OsStr::new("node"),
        &[yarn.as_os_str(), OsStr::new("install"), OsStr::new("--immutable")],
        app,
        None,
        INSTALL_LIMIT,
    )
    .await
    .map_err(|error| {
        Error::tool_failed(
            "yarn install --immutable",
            format!(
                "{error}; if the lockfile is stale after plugin changes, run `backstage-aws plugin add --install`"
            ),
        )
    })
}

pub(super) async fn prepare(
    postgres: &mut Postgres,
    app: &Path,
    args: &DevArgs,
    created: &mut Vec<String>,
) -> Result<()> {
    if args.no_install && !app.join("node_modules").is_dir() {
        return Err(Error::precondition_failed(
            "app/node_modules is missing; rerun dev without --no-install",
        ));
    }
    if create_local_config(app, args.port)? {
        created.push(LOCAL_CONFIG.to_owned());
    }
    if !app.join("node_modules").exists() && !args.no_install {
        install_dependencies(app).await?;
        created.push("node_modules".to_owned());
    }
    postgres.prepare(args.reset, created).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{EXAMPLE_CONFIG, LOCAL_CONFIG, create_local_config, exact_node_pin};

    #[test]
    fn node_pin_requires_three_numeric_components() {
        assert_eq!(exact_node_pin("v24.22.1"), Some("v24.22.1".to_owned()));
        assert_eq!(exact_node_pin("24.22.1"), Some("v24.22.1".to_owned()));
        assert_eq!(exact_node_pin("24"), None);
        assert_eq!(exact_node_pin("24.x.1"), None);
    }
    #[test]
    fn local_config_uses_requested_port_and_preserves_existing_file()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let app = directory.path();
        std::fs::write(
            app.join(EXAMPLE_CONFIG),
            "backend:\n  database:\n    connection:\n      port: 54320\n",
        )?;
        assert!(create_local_config(app, 54399)?);
        let local = app.join(LOCAL_CONFIG);
        let content = std::fs::read_to_string(&local)?;
        assert!(content.contains("port: 54399"), "{content}");
        assert!(!create_local_config(app, 54399)?);
        assert!(create_local_config(app, 54321).is_err());
        assert_eq!(std::fs::read_to_string(local)?, content);
        Ok(())
    }
}
