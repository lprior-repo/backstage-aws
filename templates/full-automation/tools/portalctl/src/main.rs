#![forbid(unsafe_code)]

use std::env;
use std::fmt;
use std::io::{self, Write};
use std::process::{Child, Command, ExitCode, ExitStatus, Stdio};
use std::thread;
use std::time::Duration;

const HELP: &str = "portalctl — optional Backstage AWS operator helper\n\n\
Usage:\n  portalctl doctor\n  portalctl rollout REGION STATE_MACHINE_ARN REQUEST_ID\n  \
portalctl status REGION EXECUTION_ARN\n\n\
Requires AWS CLI v2 with an existing authenticated profile. No credentials are stored.\n\
rollout changes AWS: it initializes the application DB and rolls out the fixed service.\n\
Use a unique request ID for each intended rollout; reuse it only for the same request.\n\
Terraform apply and image build/push remain explicit operator commands.\n";
const MAX_ARGS: usize = 4;
const MAX_ARG_BYTES: usize = 512;
const MAX_CHILD_POLLS: usize = 1200;
const CHILD_POLL: Duration = Duration::from_millis(100);

#[derive(Debug)]
enum Error {
    Usage(&'static str),
    Io(io::Error),
    ToolFailed {
        name: &'static str,
        code: Option<i32>,
    },
    Timeout(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "{message}\nRun portalctl --help for usage."),
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::ToolFailed { name, code } => write!(f, "{name} failed (exit code {code:?})"),
            Self::Timeout(name) => write!(
                f,
                "{name} exceeded the 120-second process limit; inspect AWS state before retrying"
            ),
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => match writeln!(io::stderr().lock(), "portalctl: {error}") {
            Ok(()) | Err(_) => ExitCode::FAILURE,
        },
    }
}

fn read_args() -> Result<Vec<String>, Error> {
    let mut result = Vec::with_capacity(MAX_ARGS);
    let mut arguments = env::args_os().skip(1);
    for _ in 0..MAX_ARGS {
        let Some(arg) = arguments.next() else {
            return Ok(result);
        };
        // Check the OS string before attempting a UTF-8 conversion. At most
        // four bounded arguments are retained; no unbounded collection occurs.
        if arg.as_encoded_bytes().len() > MAX_ARG_BYTES {
            return Err(Error::Usage("Argument is too long"));
        }
        let text = arg
            .into_string()
            .map_err(|_| Error::Usage("Arguments must be UTF-8"))?;
        result.push(text);
    }
    if arguments.next().is_some() {
        return Err(Error::Usage("Too many arguments"));
    }
    Ok(result)
}

fn run() -> Result<(), Error> {
    let args = read_args()?;
    match args.as_slice() {
        [] => print_help(),
        [action] if action == "--help" || action == "help" || action == "-h" => print_help(),
        [action] if action == "doctor" => doctor(),
        [action, region, arn, request_id] if action == "rollout" => {
            rollout(region, arn, request_id)
        }
        [action, region, arn] if action == "status" => status(region, arn),
        _ => Err(Error::Usage("Invalid command or argument count")),
    }
}

fn print_help() -> Result<(), Error> {
    io::stdout().lock().write_all(HELP.as_bytes())?;
    Ok(())
}

fn valid_name(value: &str, maximum: usize) -> bool {
    !value.is_empty()
        && value.len() <= maximum
        && value
            .bytes()
            .take(maximum)
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

fn valid_region(region: &str) -> bool {
    let Some((prefix, number)) = region.rsplit_once('-') else {
        return false;
    };
    region.len() <= 32
        && prefix
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_lowercase)
        && region
            .bytes()
            .take(32)
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && prefix.contains('-')
        && !region.contains("--")
        && !number.is_empty()
        && number.bytes().take(32).all(|c| c.is_ascii_digit())
}

fn validate_arn(region: &str, arn: &str, kind: &str) -> Result<(), Error> {
    if !valid_region(region) || arn.len() > MAX_ARG_BYTES {
        return Err(Error::Usage("Invalid AWS region or ARN length"));
    }
    let pieces: Vec<&str> = arn.split(':').take(9).collect();
    match pieces.as_slice() {
        [
            "arn",
            partition,
            "states",
            arn_region,
            account,
            arn_kind,
            name,
        ] if kind == "stateMachine" && *arn_kind == kind => {
            validate_arn_parts(region, partition, arn_region, account, name)
        }
        [
            "arn",
            partition,
            "states",
            arn_region,
            account,
            "execution",
            name,
            execution,
        ] if kind == "execution" && valid_name(execution, 80) => {
            validate_arn_parts(region, partition, arn_region, account, name)
        }
        _ => Err(Error::Usage(
            "Expected an unqualified Step Functions ARN of the requested type",
        )),
    }
}

fn validate_arn_parts(
    region: &str,
    partition: &str,
    arn_region: &str,
    account: &str,
    name: &str,
) -> Result<(), Error> {
    if !matches!(partition, "aws" | "aws-us-gov" | "aws-cn") || region != arn_region {
        return Err(Error::Usage("ARN partition or region does not match"));
    }
    if account.len() != 12
        || !account.bytes().take(12).all(|c| c.is_ascii_digit())
        || !valid_name(name, 80)
    {
        return Err(Error::Usage("Invalid account ID or state machine name"));
    }
    Ok(())
}

fn rollout(region: &str, arn: &str, request_id: &str) -> Result<(), Error> {
    validate_arn(region, arn, "stateMachine")?;
    if !valid_name(request_id, 64) {
        return Err(Error::Usage(
            "Request ID must contain 1–64 ASCII letters, digits, underscores or hyphens",
        ));
    }
    let name = format!("portal-{request_id}");
    let input = format!("{{\"operation\":\"rollout\",\"requestId\":\"{request_id}\"}}");
    let mut command = aws_command(region);
    command.args([
        "stepfunctions",
        "start-execution",
        "--state-machine-arn",
        arn,
        "--name",
        &name,
        "--input",
        &input,
        "--query",
        "executionArn",
        "--output",
        "text",
    ]);
    run_child("aws", &mut command)
}

fn status(region: &str, arn: &str) -> Result<(), Error> {
    validate_arn(region, arn, "execution")?;
    let mut command = aws_command(region);
    command.args([
        "stepfunctions",
        "describe-execution",
        "--execution-arn",
        arn,
        "--query",
        "{status:status,startDate:startDate,stopDate:stopDate,error:error,cause:cause}",
        "--output",
        "json",
    ]);
    run_child("aws", &mut command)
}

fn aws_command(region: &str) -> Command {
    let mut command = Command::new("aws");
    command
        .env("AWS_PAGER", "")
        .env("AWS_MAX_ATTEMPTS", "3")
        .env("AWS_RETRY_MODE", "standard");
    command.args([
        "--region",
        region,
        "--no-cli-pager",
        "--cli-connect-timeout",
        "10",
        "--cli-read-timeout",
        "60",
    ]);
    command
}

fn doctor() -> Result<(), Error> {
    for (tool, arguments) in [
        ("node", &["--version"][..]),
        ("corepack", &["--version"][..]),
        ("terraform", &["version"][..]),
        ("docker", &["version"][..]),
        ("aws", &["--version"][..]),
    ] {
        writeln!(io::stdout().lock(), "Checking {tool}:")?;
        run_child(tool, Command::new(tool).args(arguments))?;
    }
    writeln!(
        io::stdout().lock(),
        "Tools respond. See README for version pins; authentication and AWS permissions were not checked."
    )?;
    Ok(())
}

fn run_child(name: &'static str, command: &mut Command) -> Result<(), Error> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()?;
    for _ in 0..MAX_CHILD_POLLS {
        match child.try_wait() {
            Ok(Some(status)) => return check_status(name, status),
            Ok(None) => thread::sleep(CHILD_POLL),
            Err(error) => {
                stop_child(&mut child)?;
                return Err(Error::Io(error));
            }
        }
    }
    stop_child(&mut child)?;
    Err(Error::Timeout(name))
}

fn stop_child(child: &mut Child) -> Result<(), Error> {
    if child.try_wait()?.is_some() {
        return Ok(());
    }
    if let Err(error) = child.kill() {
        // The process may exit between try_wait and kill. Confirm that it has
        // exited before treating a failed kill as a cleanup failure.
        if child.try_wait()?.is_none() {
            return Err(Error::Io(error));
        }
    }
    child.wait()?;
    Ok(())
}

fn check_status(name: &'static str, status: ExitStatus) -> Result<(), Error> {
    if status.success() {
        Ok(())
    } else {
        Err(Error::ToolFailed {
            name,
            code: status.code(),
        })
    }
}
