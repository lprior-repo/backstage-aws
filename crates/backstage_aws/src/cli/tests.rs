use super::{Cli, Command};
use clap::Parser;

type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn fck_nat_flags_parse_as_global_settings() -> TestResult {
    let parsed = Cli::try_parse_from([
        "backstage-aws",
        "--fck-nat",
        "true",
        "--fck-nat-ami-id",
        "ami-1234abcd",
        "setup",
    ])?;
    assert!(matches!(parsed.command, Command::Setup(_)));
    assert!(parsed.settings.network.fck_nat);
    assert_eq!(
        parsed.settings.network.fck_nat_ami_id.as_deref(),
        Some("ami-1234abcd")
    );
    Ok(())
}

#[test]
fn fck_nat_explicit_false_is_accepted() -> TestResult {
    let explicit = Cli::try_parse_from(["backstage-aws", "--fck-nat", "false", "setup"])?;
    assert!(!explicit.settings.network.fck_nat);
    Ok(())
}
