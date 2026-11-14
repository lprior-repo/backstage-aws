use std::process::Command;

#[test]
fn help_is_available_without_aws_credentials() {
    let output = Command::new(env!("CARGO_BIN_EXE_portalctl"))
        .arg("--help")
        .output()
        .expect("execute helper");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("rollout changes AWS"));
}

#[test]
fn rejects_bad_inputs_before_running_any_external_command() {
    let cases = [
        vec![
            "rollout",
            "us-east-1",
            "arn:aws:states:eu-west-1:123456789012:stateMachine:portal",
            "request",
        ],
        vec![
            "rollout",
            "us-east-1",
            "arn:aws:states:us-east-1:123456789012:stateMachine:portal",
            "$(bad)",
        ],
        vec![
            "rollout",
            "us-east-1",
            "arn:aws:states:us-east-1:123456789012:stateMachine:portal:alias",
            "request",
        ],
        vec![
            "status",
            "us-east-1",
            "arn:aws:states:us-east-1:123456789012:stateMachine:portal",
        ],
        vec![
            "rollout",
            "--endpoint-url=evil",
            "arn:aws:states:us-east-1:123456789012:stateMachine:portal",
            "request",
        ],
        vec![
            "rollout",
            "--profile",
            "arn:aws:states:--profile:123456789012:stateMachine:portal",
            "request",
        ],
        vec![
            "rollout",
            "us-east-1",
            "arn:aws:states:us-east-1:123456789012:stateMachine:portal",
            "",
        ],
        vec![
            "rollout",
            "us-east-1",
            "arn:aws:states:us-east-1:123456789012:stateMachine:portal",
            "request",
            "extra",
        ],
    ];
    for args in cases {
        let output = Command::new(env!("CARGO_BIN_EXE_portalctl"))
            .args(args)
            .env("PATH", "/nonexistent")
            .output()
            .expect("execute helper");
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("I/O error"),
            "must reject before spawning aws: {stderr}"
        );
    }
}

#[test]
fn oversized_arguments_are_rejected_before_external_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_portalctl"))
        .arg("x".repeat(513))
        .env("PATH", "/nonexistent")
        .output()
        .expect("execute helper");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Argument is too long"));
}

#[cfg(unix)]
mod unix {
    use super::Command;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_FIXTURE: AtomicUsize = AtomicUsize::new(0);

    struct FakeAws(PathBuf);

    impl FakeAws {
        fn new(binary: &str) -> Self {
            let number = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/test-fixtures")
                .join(format!("{}-{number}", std::process::id()));
            fs::create_dir_all(&directory).expect("create isolated test fixture");
            symlink(binary, directory.join("aws")).expect("link controlled test command");
            Self(directory)
        }

        fn command(&self) -> Command {
            let mut command = Command::new(env!("CARGO_BIN_EXE_portalctl"));
            command.env("PATH", &self.0);
            command
        }
    }

    impl Drop for FakeAws {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove isolated test fixture");
        }
    }

    #[test]
    fn rollout_preserves_the_retry_identity_and_passes_data_as_arguments() {
        // echo receives the real argv through exec, so no AWS credentials or
        // network calls are possible. Repeating a request must retain the name
        // and exact input that Step Functions uses for deduplication.
        let fake = FakeAws::new("/bin/echo");
        let arguments = [
            "rollout",
            "us-east-1",
            "arn:aws:states:us-east-1:123456789012:stateMachine:portal",
            "same-request_42",
        ];
        let first = fake.command().args(arguments).output().expect("run helper");
        let second = fake
            .command()
            .args(arguments)
            .output()
            .expect("retry helper");
        assert!(first.status.success());
        assert!(second.status.success());
        assert_eq!(first.stdout, second.stdout);
        let stdout = String::from_utf8_lossy(&first.stdout);
        assert!(stdout.contains("--name portal-same-request_42"));
        assert!(
            stdout
                .contains("--input {\"operation\":\"rollout\",\"requestId\":\"same-request_42\"}")
        );
        assert!(stdout.contains(
            "--state-machine-arn arn:aws:states:us-east-1:123456789012:stateMachine:portal"
        ));
    }

    #[test]
    fn external_command_failure_is_reported_as_failure() {
        let fake = FakeAws::new("/bin/false");
        let output = fake
            .command()
            .args([
                "status",
                "us-east-1",
                "arn:aws:states:us-east-1:123456789012:execution:portal:portal-request",
            ])
            .output()
            .expect("run helper");
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("aws failed (exit code Some(1))"));
    }

    #[test]
    fn non_utf8_arguments_fail_without_panicking() {
        let output = Command::new(env!("CARGO_BIN_EXE_portalctl"))
            .arg(OsString::from_vec(vec![0xff]))
            .env("PATH", "/nonexistent")
            .output()
            .expect("execute helper");
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("Arguments must be UTF-8"));
    }
}
