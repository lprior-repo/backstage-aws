#![forbid(unsafe_code)]
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    backstage_aws::run_cli().await
}
