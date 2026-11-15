use super::{Cli, Command};
use clap::Parser;

#[test]
fn fck_nat_flags_parse_as_global_settings() {
    let parsed = Cli::try_parse_from([
        "backstage-aws",
        "--fck-nat",
        "true",
        "--fck-nat-ami-id",
        "ami-1234abcd",
        "setup",
    ])
    .expect("valid fck-nat arguments");
    assert!(matches!(parsed.command, Command::Setup(_)));
    assert!(parsed.settings.network.fck_nat);
    assert_eq!(
        parsed.settings.network.fck_nat_ami_id.as_deref(),
        Some("ami-1234abcd")
    );
}

#[test]
fn fck_nat_defaults_off_and_accepts_explicit_false() {
    let default = Cli::try_parse_from(["backstage-aws", "setup"]).expect("default settings");
    assert!(!default.settings.network.fck_nat);
    assert_eq!(default.settings.network.fck_nat_ami_id, None);

    let explicit = Cli::try_parse_from(["backstage-aws", "--fck-nat", "false", "setup"])
        .expect("explicit false setting");
    assert!(!explicit.settings.network.fck_nat);
}
