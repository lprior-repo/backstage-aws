use backstage_aws::{Config, generate, read_config};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const EXAMPLE: &str = include_str!("../../../generator.example.toml");
static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn sandbox(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let id = NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed);
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.cache/rust-tests")
        .join(format!("{name}-{}-{id}", std::process::id()));
    fs::create_dir_all(&path)?;
    Ok(path)
}
fn files(root: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut result = Vec::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if path.is_dir() {
            result.extend(files(&path)?);
        } else {
            result.push(path);
        }
    }
    result.sort();
    Ok(result)
}

#[test]
fn generates_full_automation_with_concrete_configuration() -> Result<(), Box<dyn Error>> {
    let root = sandbox("automation-complete")?.join("portal");
    let count = generate(&Config::parse(EXAMPLE)?, &root)?;
    assert_eq!(count, 144);
    assert_eq!(files(&root)?.len(), count);
    assert_eq!(
        fs::read(root.join("app/.yarn/releases/yarn-4.13.0.cjs"))?,
        include_bytes!("../../../templates/full-automation/app/.yarn/releases/yarn-4.13.0.cjs")
    );
    assert_eq!(
        fs::read(root.join("app/packages/app/public/favicon.ico"))?,
        include_bytes!("../../../templates/full-automation/app/packages/app/public/favicon.ico")
    );
    assert_eq!(
        fs::read(root.join("app/yarn.lock"))?,
        include_bytes!("../../../templates/full-automation/app/yarn.lock")
    );
    let deployment: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("deployment.json"))?)?;
    assert_eq!(deployment["name"], "platform-portal");
    assert_eq!(deployment["awsAccountId"], "123456789012");
    assert_eq!(deployment["awsRegion"], "us-east-1");
    assert_eq!(deployment["domainName"], "backstage.example.com");
    assert_eq!(
        deployment["githubRepository"],
        "example-org/platform-portal"
    );
    assert_eq!(deployment["ownerGithubId"], "12345");
    assert_eq!(
        deployment["allowedIngressCidrs"],
        serde_json::json!(["203.0.113.10/32"])
    );
    assert_eq!(deployment["portalDesiredCount"], 2);
    assert_eq!(deployment["maximumCriticalFindings"], 0);
    assert_eq!(deployment["accountBaseline"], true);
    assert!(
        fs::read_to_string(root.join("app/catalog/org.yaml"))?
            .contains("github.com/user-id: '12345'")
    );
    assert!(
        fs::read_to_string(root.join("app/app-config.yaml"))?.contains("title: platform-portal")
    );
    assert!(root.join("automation/cli.mjs").is_file());
    assert!(root.join("infra/core/workflow.tf").is_file());
    assert!(root.join("infra/core/maintenance.tf").is_file());
    assert!(root.join("tools/portalctl/Cargo.toml").is_file());
    assert!(root.join("provenance/archive-SOURCE-SHA256SUMS").is_file());
    assert!(!root.join("SOURCE-SHA256SUMS").exists());
    assert!(!root.join("Cargo.toml").exists());
    let workflow = fs::read_to_string(root.join(".github/workflows/deploy.yml"))?;
    assert!(workflow.contains("${{ "));
    assert!(fs::read_to_string(root.join("infra/core/workflow.tf"))?.contains("JSONata"));
    for path in files(&root)? {
        if let Ok(text) = fs::read_to_string(path) {
            assert!(!text.contains("__BACKSTAGE_AWS_"));
        }
    }
    Ok(())
}

#[test]
fn renders_two_independent_configurations_deterministically() -> Result<(), Box<dyn Error>> {
    let root = sandbox("automation-determinism")?;
    let first = root.join("first");
    let second = root.join("second");
    let third = root.join("third");
    let config = Config::parse(EXAMPLE)?;
    generate(&config, &first)?;
    generate(&config, &second)?;
    for path in files(&first)? {
        assert_eq!(
            fs::read(&path)?,
            fs::read(second.join(path.strip_prefix(&first)?))?
        );
    }
    let other = EXAMPLE
        .replace("platform-portal", "another-portal")
        .replace("12345\"", "23456\"")
        .replace("us-east-1", "eu-west-1")
        .replace("123456789012", "234567890123")
        .replace("backstage.example.com", "another.example.org")
        .replace(
            "[\"203.0.113.10/32\"]",
            "[\"192.0.2.0/24\", \"198.51.100.0/24\"]",
        );
    generate(&Config::parse(&other)?, &third)?;
    let deployment: serde_json::Value =
        serde_json::from_slice(&fs::read(third.join("deployment.json"))?)?;
    assert_eq!(deployment["name"], "another-portal");
    assert_eq!(deployment["awsAccountId"], "234567890123");
    assert_eq!(deployment["awsRegion"], "eu-west-1");
    assert_eq!(deployment["ownerGithubId"], "23456");
    assert_eq!(
        deployment["allowedIngressCidrs"],
        serde_json::json!(["192.0.2.0/24", "198.51.100.0/24"])
    );
    assert!(
        fs::read_to_string(third.join("app/catalog/org.yaml"))?
            .contains("github.com/user-id: '23456'")
    );
    Ok(())
}

#[test]
fn copied_binary_works_offline_outside_repository() -> Result<(), Box<dyn Error>> {
    let root = sandbox("automation-standalone")?;
    let binary = root.join("backstage-aws");
    fs::copy(env!("CARGO_BIN_EXE_backstage-aws"), &binary)?;
    fs::write(root.join("config.toml"), EXAMPLE)?;
    let result = Command::new(binary)
        .current_dir(&root)
        .env("PATH", "")
        .args(["config.toml", "output"])
        .output()?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("Generated 144 files"));
    assert!(root.join("output/platform").is_file());
    assert!(root.join("output/deployment.json").is_file());
    Ok(())
}

#[cfg(unix)]
#[test]
fn generated_shell_entry_points_are_executable() -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::PermissionsExt;
    let root = sandbox("permissions")?.join("output");
    generate(&Config::parse(EXAMPLE)?, &root)?;
    for file in [
        "platform",
        "scripts/check-automation.sh",
        "scripts/provision.sh",
    ] {
        assert_ne!(
            fs::metadata(root.join(file))?.permissions().mode() & 0o111,
            0
        );
    }
    assert_eq!(
        fs::metadata(root.join("deployment.json"))?
            .permissions()
            .mode()
            & 0o111,
        0
    );
    Ok(())
}

#[test]
fn existing_directory_and_file_are_never_overwritten() -> Result<(), Box<dyn Error>> {
    let root = sandbox("preserve")?;
    let config = Config::parse(EXAMPLE)?;
    let directory = root.join("existing");
    fs::create_dir(&directory)?;
    fs::write(directory.join("user-data"), "preserve me")?;
    assert!(generate(&config, &directory).is_err());
    assert_eq!(
        fs::read_to_string(directory.join("user-data"))?,
        "preserve me"
    );
    assert_eq!(files(&directory)?.len(), 1);
    let file = root.join("existing-file");
    fs::write(&file, "preserve file")?;
    assert!(generate(&config, &file).is_err());
    assert_eq!(fs::read_to_string(file)?, "preserve file");
    let empty = root.join("empty");
    fs::create_dir(&empty)?;
    assert!(generate(&config, &empty).is_err());
    assert!(files(&empty)?.is_empty());
    Ok(())
}

#[cfg(unix)]
#[test]
fn output_symlink_is_rejected_without_touching_its_target() -> Result<(), Box<dyn Error>> {
    let root = sandbox("symlink")?;
    let target = root.join("target");
    fs::create_dir(&target)?;
    fs::write(target.join("user-data"), "unchanged")?;
    let link = root.join("output");
    std::os::unix::fs::symlink(&target, &link)?;
    assert!(generate(&Config::parse(EXAMPLE)?, &link).is_err());
    assert_eq!(files(&target)?.len(), 1);
    assert_eq!(fs::read_to_string(target.join("user-data"))?, "unchanged");
    Ok(())
}

#[test]
fn rejects_invalid_inputs_and_template_injection() {
    let cases = [
        ("platform-portal\"", "bad\"name\""),
        ("platform-portal\"", "${file(\"secret\")}\""),
        ("platform-portal\"", "../escape\""),
        ("backstage.example.com", "https://backstage.example.com"),
        ("backstage.example.com", "bad_host.example.com"),
        ("us-east-1", "cn-north-1"),
        ("us-east-1", "us-gov-west-1"),
        ("123456789012", "not-an-account"),
        ("Z1234567890", "ZONE\nINJECTION"),
        ("example-org/platform-portal", "../repository"),
        ("12345\"", "login-not-id\""),
        ("12345\"", "0\""),
        ("12345\"", "9007199254740992\""),
        ("203.0.113.10/32", "203.0.113.10/33"),
        ("203.0.113.10/32", "203.0.113.10/+32"),
        ("203.0.113.10/32", "0.0.0.0/0"),
        ("203.0.113.10/32", "300.0.113.10/32"),
        ("[\"203.0.113.10/32\"]", "[]"),
        ("operator@example.com", "invalid-email"),
        ("monthly_budget_usd = 300", "monthly_budget_usd = 0"),
        ("portal_desired_count = 2", "portal_desired_count = 5"),
        ("account_baseline = true", "account_baseline = \"true\""),
    ];
    for (before, after) in cases {
        assert!(EXAMPLE.contains(before), "invalid fixture: {before}");
        assert!(
            Config::parse(&EXAMPLE.replace(before, after)).is_err(),
            "accepted {after}"
        );
    }
    assert!(Config::parse("name = \"missing-fields\"").is_err());
}

#[test]
fn invalid_config_leaves_no_output_and_does_not_echo_secret_contents() -> Result<(), Box<dyn Error>>
{
    let root = sandbox("invalid")?;
    let secret = "DO-NOT-PRINT-THIS-SECRET";
    let path = root.join("bad.toml");
    fs::write(&path, format!("{EXAMPLE}\ngithub_token = \"{secret}\"\n"))?;
    let result = Command::new(env!("CARGO_BIN_EXE_backstage-aws"))
        .arg(&path)
        .arg(root.join("output"))
        .output()?;
    assert!(!result.status.success());
    assert!(!root.join("output").exists());
    assert!(!String::from_utf8_lossy(&result.stderr).contains(secret));
    assert!(String::from_utf8_lossy(&result.stderr).contains("invalid configuration"));
    assert!(result.stdout.is_empty());
    Ok(())
}

#[test]
fn oversized_and_missing_configuration_files_fail_before_generation() -> Result<(), Box<dyn Error>>
{
    let root = sandbox("bounded")?;
    let large = root.join("oversized.toml");
    fs::write(&large, " ".repeat(65_537))?;
    assert!(read_config(&large).is_err());
    assert!(read_config(&root.join("missing.toml")).is_err());
    assert!(Config::parse(&" ".repeat(65_537)).is_err());
    Ok(())
}

#[test]
fn cli_help_and_wrong_arguments_have_explicit_exit_statuses() -> Result<(), Box<dyn Error>> {
    let binary = env!("CARGO_BIN_EXE_backstage-aws");
    assert!(
        Command::new(binary)
            .arg("--help")
            .output()?
            .status
            .success()
    );
    let missing = Command::new(binary).output()?;
    assert!(!missing.status.success());
    assert!(String::from_utf8_lossy(&missing.stderr).contains("usage:"));
    assert!(
        !Command::new(binary)
            .args(["a", "b", "extra"])
            .output()?
            .status
            .success()
    );
    Ok(())
}
