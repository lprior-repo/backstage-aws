use super::{get, remove_text, set};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use tempfile::TempDir;

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

fn app_with(source: &str) -> TestResult<(TempDir, PathBuf)> {
    let directory = tempfile::tempdir()?;
    let app = directory.path().join("app");
    fs::create_dir(&app)?;
    fs::write(app.join("app-config.yaml"), source)?;
    Ok((directory, app))
}

#[test]
fn setting_a_template_origin_changes_only_that_value() -> TestResult {
    let source = include_str!("../../../../template/app/app-config.yaml");
    let expected = source.replacen(
        "origin: http://localhost:3000",
        "origin: https://preview.example.test",
        1,
    );

    let (_directory, app) = app_with(source)?;
    let result = set(
        &app,
        "app-config.yaml",
        "backend.cors.origin",
        "https://preview.example.test",
    )?;
    let updated = fs::read_to_string(app.join("app-config.yaml"))?;

    assert_eq!(result.get("changed"), Some(&Value::Bool(true)));
    assert_eq!(updated, expected);
    Ok(())
}

#[test]
fn setting_preserves_comments_and_existing_neighbours() -> TestResult {
    let source = "# project header\nbackend:\n  cors:\n    origin: http://old.example # keep this note\n    credentials: true\n  baseUrl: http://backend.example\n";
    let (_directory, app) = app_with(source)?;

    let result = set(
        &app,
        "app-config.yaml",
        "backend.cors.origin",
        "http://new.example",
    )?;
    let updated = fs::read_to_string(app.join("app-config.yaml"))?;

    assert_eq!(result.get("changed"), Some(&Value::Bool(true)));
    assert_eq!(result.get("value"), Some(&json!("http://new.example")));
    assert!(updated.contains("# project header"));
    assert!(updated.contains("origin: http://new.example # keep this note"));
    assert!(updated.contains("credentials: true"));
    assert!(updated.contains("baseUrl: http://backend.example"));
    Ok(())
}

#[test]
fn nested_creation_and_typed_values_round_trip_through_config_commands() -> TestResult {
    let (_directory, app) = app_with("app:\n  title: Example\n")?;

    set(&app, "app-config.yaml", "backend.cors.enabled", "true")?;
    set(&app, "app-config.yaml", "backend.cors.port", "42")?;
    set(
        &app,
        "app-config.yaml",
        "backend.cors.label",
        "local development",
    )?;
    set(
        &app,
        "app-config.yaml",
        "backend.cors.origin",
        "${BACKEND_ORIGIN}",
    )?;

    let enabled = get(&app, "app-config.yaml", "backend.cors.enabled")?;
    let port = get(&app, "app-config.yaml", "backend.cors.port")?;
    let label = get(&app, "app-config.yaml", "backend.cors.label")?;
    let origin = get(&app, "app-config.yaml", "backend.cors.origin")?;

    assert_eq!(enabled.get("value"), Some(&Value::Bool(true)));
    assert_eq!(port.get("value"), Some(&json!(42)));
    assert_eq!(label.get("value"), Some(&json!("local development")));
    assert_eq!(origin.get("value"), Some(&json!("${BACKEND_ORIGIN}")));
    let updated = fs::read_to_string(app.join("app-config.yaml"))?;
    assert!(updated.contains("title: Example"));
    assert!(updated.contains("origin: ${BACKEND_ORIGIN}"));
    Ok(())
}

#[test]
fn get_converts_mappings_sequences_and_scalar_types_to_json() -> TestResult {
    let source =
        "backend:\n  cors:\n    methods: [GET, POST]\n    credentials: true\n    port: 7007\n";
    let (_directory, app) = app_with(source)?;

    let result = get(&app, "app-config.yaml", "backend.cors")?;

    assert_eq!(result.get("found"), Some(&Value::Bool(true)));
    assert_eq!(
        result.get("value"),
        Some(&json!({
            "methods": ["GET", "POST"],
            "credentials": true,
            "port": 7007,
        }))
    );
    Ok(())
}

#[test]
fn get_reports_missing_paths_without_failing() -> TestResult {
    let (_directory, app) = app_with("backend:\n  baseUrl: http://localhost:7007\n")?;

    let result = get(&app, "app-config.yaml", "backend.cors.origin")?;

    assert_eq!(result.get("found"), Some(&Value::Bool(false)));
    assert_eq!(result.get("value"), Some(&Value::Null));
    Ok(())
}

#[test]
fn removing_an_absent_path_is_an_exact_no_op() -> TestResult {
    let source = "# keep\nbackend:\n  baseUrl: http://localhost:7007\n";

    let updated = remove_text(source, "backend.cors.origin")?;

    assert_eq!(updated, source);
    Ok(())
}

#[test]
fn removing_an_existing_path_keeps_its_siblings() -> TestResult {
    let source = "backend:\n  cors:\n    origin: http://localhost:3000\n    credentials: true\n";

    let updated = remove_text(source, "backend.cors.origin")?;

    assert!(!updated.contains("origin:"));
    assert!(updated.contains("credentials: true"));
    Ok(())
}

#[test]
fn setting_an_unchanged_value_does_not_report_a_write() -> TestResult {
    let source = "backend:\n  cors:\n    origin: http://localhost:3000\n";
    let (_directory, app) = app_with(source)?;

    let result = set(
        &app,
        "app-config.yaml",
        "backend.cors.origin",
        "http://localhost:3000",
    )?;

    assert_eq!(result.get("changed"), Some(&Value::Bool(false)));
    assert_eq!(fs::read_to_string(app.join("app-config.yaml"))?, source);
    Ok(())
}

#[test]
fn invalid_file_names_are_rejected() -> TestResult {
    let (_directory, app) = app_with("backend: {}\n")?;

    [
        "../outside.yaml",
        "/tmp/outside.yaml",
        "nested/config.yaml",
        "nested\\config.yaml",
        "config.json",
        "prefix..yaml",
        ".yaml",
    ]
    .into_iter()
    .try_for_each(|file| {
        let result = get(&app, file, "backend");
        assert_eq!(
            result.map(|_| ()).map_err(|error| error.code()),
            Err("INVALID_INPUT")
        );
        Ok::<(), Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}

#[test]
fn missing_files_and_malformed_yaml_have_actionable_errors() -> TestResult {
    let directory = tempfile::tempdir()?;
    let app = directory.path().join("app");
    fs::create_dir(&app)?;

    let missing = get(&app, "missing.yaml", "backend");
    assert_eq!(
        missing.map(|_| ()).map_err(|error| error.code()),
        Err("PRECONDITION_FAILED")
    );

    fs::write(app.join("app-config.yaml"), "backend: [unclosed\n")?;
    let invalid_yaml = get(&app, "app-config.yaml", "backend");
    assert_eq!(
        invalid_yaml.map(|_| ()).map_err(|error| error.code()),
        Err("INVALID_INPUT")
    );
    Ok(())
}
