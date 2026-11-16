use super::{EditArgs, TestResult, add, dependencies, fixture, plugin_args};
use serde_json::Value;
use std::{fs, io};

#[tokio::test]
async fn unknown_package_requires_an_exact_version() -> TestResult {
    // Given: an app based on the template.
    let fixture = fixture()?;
    let args = EditArgs {
        frontend: vec!["@backstage/plugin-not-in-release".to_owned()],
        ..EditArgs::default()
    };
    // When: an unknown package is requested without a pinned or explicit version.
    let error = add(&fixture.project, &args)
        .await
        .err()
        .ok_or_else(|| io::Error::other("unknown package without version must be refused"))?;
    // Then: the user receives a precise invalid-input hint.
    assert_eq!(error.code(), "INVALID_INPUT");
    assert!(
        error
            .to_string()
            .contains("@backstage/plugin-not-in-release@VER")
    );
    Ok(())
}

#[tokio::test]
async fn dry_run_reports_planned_edits_without_writing() -> TestResult {
    // Given: a template app and its original package manifest bytes.
    let fixture = fixture()?;
    let manifest = fixture.app.join("packages/app/package.json");
    let original = fs::read(&manifest)?;
    let args = EditArgs {
        dry_run: true,
        ..plugin_args("api-docs")
    };
    // When: the catalog plugin is added with dry-run enabled.
    let result = add(&fixture.project, &args).await?;
    // Then: a planned change is reported without writing.
    assert_eq!(result.get("dry_run").and_then(Value::as_bool), Some(true));
    assert_eq!(result.get("changed").and_then(Value::as_bool), Some(true));
    assert_eq!(fs::read(&manifest)?, original);
    Ok(())
}

#[tokio::test]
async fn unknown_package_with_exact_explicit_version_is_added() -> TestResult {
    // Given: a template app without release metadata and an explicit version.
    let fixture = fixture()?;
    fs::remove_file(fixture.app.join("backstage.json"))?;
    let manifest = fixture.app.join("packages/app/package.json");
    let args = EditArgs {
        frontend: vec!["@company/plugin-custom@1.2.3".to_owned()],
        ..EditArgs::default()
    };
    // When: an unknown package is supplied with an exact version.
    let result = add(&fixture.project, &args).await?;
    // Then: the explicit version is inserted.
    assert_eq!(result.get("changed").and_then(Value::as_bool), Some(true));
    assert_eq!(
        dependencies(&manifest)?
            .get("@company/plugin-custom")
            .and_then(Value::as_str),
        Some("1.2.3")
    );
    Ok(())
}

#[tokio::test]
async fn explicit_package_version_must_be_exact_semver() -> TestResult {
    // Given: a template app and its original package manifest bytes.
    let fixture = fixture()?;
    let manifest = fixture.app.join("packages/app/package.json");
    let original = fs::read(&manifest)?;
    let args = EditArgs {
        frontend: vec!["@company/plugin-custom@^1.2.3".to_owned()],
        ..EditArgs::default()
    };
    // When: a non-exact version is requested.
    let error = add(&fixture.project, &args)
        .await
        .err()
        .ok_or_else(|| io::Error::other("ranges are not exact versions"))?;
    // Then: the request is rejected without changing the manifest.
    assert_eq!(error.code(), "INVALID_INPUT");
    assert_eq!(fs::read(manifest)?, original);
    Ok(())
}
