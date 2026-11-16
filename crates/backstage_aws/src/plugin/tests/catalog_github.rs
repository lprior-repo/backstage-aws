use super::{EditArgs, TestResult, add, fixture, remove};
use serde_json::Value;

#[tokio::test]
async fn catalog_github_sets_provider_defaults_and_removes_its_config() -> TestResult {
    // Given: a fresh template app and a named GitHub organization.
    let fixture = fixture()?;
    let args = EditArgs {
        ids: vec!["catalog-github".to_owned()],
        param: vec!["org=example-org".to_owned()],
        ..EditArgs::default()
    };

    // When: the GitHub catalog plugin is added.
    add(&fixture.project, &args).await?;

    // Then: its provider settings are stored with typed values.
    let config = |path| crate::app_config::get(&fixture.app, "app-config.yaml", path);
    assert_eq!(
        config("catalog.providers.github.main.organization")?
            .get("value")
            .and_then(Value::as_str),
        Some("example-org")
    );
    assert_eq!(
        config("catalog.providers.github.main.catalogPath")?
            .get("value")
            .and_then(Value::as_str),
        Some("/catalog-info.yaml")
    );
    assert_eq!(
        config("catalog.providers.github.main.filters.branch")?
            .get("value")
            .and_then(Value::as_str),
        Some("main")
    );
    assert_eq!(
        config("catalog.providers.github.main.schedule.frequency.minutes")?
            .get("value")
            .and_then(Value::as_u64),
        Some(30)
    );
    assert_eq!(
        config("catalog.providers.github.main.schedule.timeout.minutes")?
            .get("value")
            .and_then(Value::as_u64),
        Some(3)
    );

    // When: the plugin is removed.
    remove(&fixture.project, &super::plugin_args("catalog-github")).await?;

    // Then: the plugin's organization setting is gone.
    assert_eq!(
        config("catalog.providers.github.main.organization")?
            .get("found")
            .and_then(Value::as_bool),
        Some(false)
    );
    Ok(())
}
