use super::{EditArgs, add, list, remove};
use serde_json::Value;
use std::{
    error::Error as StdError,
    fs, io,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

type TestResult<T = ()> = Result<T, Box<dyn StdError>>;

#[path = "tests/catalog_github.rs"]
mod catalog_github;
#[path = "tests/edge_cases.rs"]
mod edge_cases;

struct Fixture {
    _temp: TempDir,
    project: PathBuf,
    app: PathBuf,
}

fn fixture() -> TestResult<Fixture> {
    let temp = tempfile::tempdir()?;
    let project = temp.path().join("project");
    let app = project.join("app");
    let template = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../template/app");
    let release: Value = serde_json::from_slice(&fs::read(template.join("backstage.json"))?)?;
    let version = release
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| io::Error::other("template release version"))?;
    let files = [
        "package.json",
        "backstage.json",
        "app-config.yaml",
        "packages/app/package.json",
        "packages/app/src/modules/nav/Sidebar.tsx",
        "packages/backend/package.json",
        "packages/backend/src/index.ts",
    ];
    for relative in files {
        copy_template_file(&template, &app, relative)?;
    }
    copy_template_file(
        &template,
        &app,
        &format!("provenance/backstage-release-{version}.json"),
    )?;
    Ok(Fixture {
        _temp: temp,
        project,
        app,
    })
}

fn copy_template_file(template: &Path, app: &Path, relative: &str) -> TestResult {
    let source = template.join(relative);
    let destination = app.join(relative);
    let parent = destination
        .parent()
        .ok_or_else(|| io::Error::other("fixture destination parent"))?;
    fs::create_dir_all(parent)?;
    fs::copy(source, destination)?;
    Ok(())
}

fn plugin_args(id: &str) -> EditArgs {
    EditArgs {
        ids: vec![id.to_owned()],
        ..EditArgs::default()
    }
}

fn dependencies(path: &Path) -> TestResult<Value> {
    let manifest: Value = serde_json::from_slice(&fs::read(path)?)?;
    manifest
        .get("dependencies")
        .cloned()
        .ok_or_else(|| io::Error::other("dependency object").into())
}

fn listed_plugin(project: &Path, id: &str) -> TestResult<Value> {
    let listed = list(project)?;
    listed
        .get("plugins")
        .and_then(Value::as_array)
        .and_then(|plugins| {
            plugins
                .iter()
                .find(|plugin| plugin.get("id").and_then(Value::as_str) == Some(id))
        })
        .cloned()
        .ok_or_else(|| io::Error::other("catalog plugin entry").into())
}

#[tokio::test]
async fn add_is_idempotent_sorted_and_remove_restores_original_manifest() -> TestResult {
    // Given: a fresh copy of the real template app.
    let fixture = fixture()?;
    let manifest = fixture.app.join("packages/app/package.json");
    let original = fs::read(&manifest)?;
    // When: add the catalog API docs plugin.
    let result = add(&fixture.project, &plugin_args("api-docs")).await?;
    // Then: the dependency is sorted into the app manifest.
    assert_eq!(result.get("changed").and_then(Value::as_bool), Some(true));
    let changed = fs::read_to_string(&manifest)?;
    let api_docs = changed
        .find("\"@backstage/plugin-api-docs\"")
        .ok_or_else(|| io::Error::other("sorted plugin line"))?;
    let app_module = changed
        .find("\"@backstage/plugin-app-module-user-settings\"")
        .ok_or_else(|| io::Error::other("next package line"))?;
    assert!(api_docs < app_module);
    // When: repeat add, then remove the plugin.
    let second = add(&fixture.project, &plugin_args("api-docs")).await?;
    // Then: add is idempotent and remove restores the original manifest bytes.
    assert_eq!(second.get("changed").and_then(Value::as_bool), Some(false));
    assert_eq!(fs::read(&manifest)?, changed.as_bytes());
    remove(&fixture.project, &plugin_args("api-docs")).await?;
    assert_eq!(fs::read(&manifest)?, original);
    Ok(())
}

fn remove_template_signals(fixture: &Fixture) -> TestResult {
    let frontend = fixture.app.join("packages/app/package.json");
    let frontend_text = fs::read_to_string(&frontend)?;
    let frontend_without_signals =
        super::dependencies::remove_dependency(&frontend_text, "@backstage/plugin-signals")?;
    fs::write(&frontend, frontend_without_signals)?;

    let backend = fixture.app.join("packages/backend/package.json");
    let backend_text = fs::read_to_string(&backend)?;
    let backend_without_signals =
        super::dependencies::remove_dependency(&backend_text, "@backstage/plugin-signals-backend")?;
    fs::write(&backend, backend_without_signals)?;

    let index = fixture.app.join("packages/backend/src/index.ts");
    let index_text = fs::read_to_string(&index)?;
    let index_without_signals = super::source_edits::remove_backend_registration(
        &index_text,
        "@backstage/plugin-signals-backend",
    )?;
    fs::write(index, index_without_signals)?;
    Ok(())
}

#[tokio::test]
async fn notifications_adds_its_signals_requirements_and_sidebar_patch() -> TestResult {
    // Given: the template app has no signals dependency or backend registration.
    let fixture = fixture()?;
    remove_template_signals(&fixture)?;
    let frontend = fixture.app.join("packages/app/package.json");
    let backend = fixture.app.join("packages/backend/package.json");
    // When: add notifications from the catalog.
    add(&fixture.project, &plugin_args("notifications")).await?;
    // Then: notifications and its required signals packages are registered with pinned versions.
    let frontend_dependencies = dependencies(&frontend)?;
    let backend_dependencies = dependencies(&backend)?;
    assert_eq!(
        frontend_dependencies
            .get("@backstage/plugin-signals")
            .and_then(Value::as_str),
        Some("0.0.36")
    );
    assert!(
        frontend_dependencies
            .get("@backstage/plugin-notifications")
            .is_some()
    );
    assert_eq!(
        backend_dependencies
            .get("@backstage/plugin-signals-backend")
            .and_then(Value::as_str),
        Some("0.3.20")
    );
    assert!(
        backend_dependencies
            .get("@backstage/plugin-notifications-backend")
            .is_some()
    );
    let index = fs::read_to_string(fixture.app.join("packages/backend/src/index.ts"))?;
    assert!(index.contains("backend.add(import('@backstage/plugin-signals-backend'));"));
    assert!(index.contains("backend.add(import('@backstage/plugin-notifications-backend'));"));
    let sidebar = fs::read_to_string(fixture.app.join("packages/app/src/modules/nav/Sidebar.tsx"))?;
    assert!(
        sidebar.contains(
            "import { NotificationsSidebarItem } from '@backstage/plugin-notifications';"
        )
    );
    assert!(sidebar.contains("            <NotificationsSidebarItem />"));
    let notifications = listed_plugin(&fixture.project, "notifications")?;
    assert_eq!(
        notifications.get("installed").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        notifications
            .get("requires")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    Ok(())
}

#[tokio::test]
async fn missing_notifications_sidebar_anchor_refuses_without_writing_other_files() -> TestResult {
    // Given: the template's notifications import anchor is missing.
    let fixture = fixture()?;
    let sidebar_path = fixture.app.join("packages/app/src/modules/nav/Sidebar.tsx");
    let sidebar = fs::read_to_string(&sidebar_path)?;
    let broken = sidebar.replace(
        "import { UserSettingsSignInAvatar } from '@backstage/plugin-user-settings';",
        "",
    );
    assert_ne!(broken, sidebar);
    fs::write(&sidebar_path, broken)?;
    let frontend_path = fixture.app.join("packages/app/package.json");
    let backend_path = fixture.app.join("packages/backend/package.json");
    let index_path = fixture.app.join("packages/backend/src/index.ts");
    let before = [
        fs::read(&frontend_path)?,
        fs::read(&backend_path)?,
        fs::read(&index_path)?,
    ];
    // When: notifications are added.
    let error = add(&fixture.project, &plugin_args("notifications"))
        .await
        .err()
        .ok_or_else(|| io::Error::other("missing sidebar anchor must refuse the entire edit"))?;
    // Then: the manual precondition error is returned before any other file changes.
    assert_eq!(error.code(), "PRECONDITION_FAILED");
    assert!(error.to_string().contains("NotificationsSidebarItem />"));
    let [frontend_before, backend_before, index_before] = before;
    assert_eq!(fs::read(frontend_path)?, frontend_before);
    assert_eq!(fs::read(backend_path)?, backend_before);
    assert_eq!(fs::read(index_path)?, index_before);
    Ok(())
}
