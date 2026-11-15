#![forbid(unsafe_code)]
pub mod check;
pub mod cli;
pub mod ctx;
pub mod db;
pub mod error;
pub mod github;
pub mod onboarding;
pub mod ops;
pub mod process;
pub mod settings;
pub mod template;
pub mod terraform;
use crate::{
    cli::{Cli, command_name},
    ctx::{Ctx, print_failure, print_success},
    error::{Error, Result},
};
use aws_config::BehaviorVersion;
use clap::{Parser, error::ErrorKind};
use std::process::ExitCode;

pub async fn run_cli() -> ExitCode {
    let args = std::env::args_os().collect::<Vec<_>>();
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                return if error.print().is_ok() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(4)
                };
            }
            return output_failure("unknown", &Error::invalid_input(error.to_string()));
        }
    };
    let name = command_name(&cli.command);
    match Box::pin(run(cli)).await {
        Ok(result) => match print_success(name, result) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => ExitCode::from(error.exit_code()),
        },
        Err(error) => output_failure(name, &error),
    }
}

fn output_failure(command: &str, error: &Error) -> ExitCode {
    match print_failure(command, error) {
        Ok(()) => ExitCode::from(error.exit_code()),
        Err(_) => ExitCode::from(4),
    }
}

async fn run(cli: Cli) -> Result<serde_json::Value> {
    if let cli::Command::Setup(args) = &cli.command {
        return onboarding::run(&cli.settings, args).await;
    }
    if let cli::Command::Check { app } = &cli.command {
        // Offline: needs neither deployment settings nor credentials.
        let terraform = cli.settings.terraform_path();
        terraform::verify_version(&terraform).await?;
        let checked = check::check(&cli.settings.project, &terraform, *app).await?;
        return serde_json::to_value(checked)
            .map_err(|error| Error::state_failed(error.to_string()));
    }
    let settings = cli.settings.validate()?;
    let region = aws_config::Region::new(settings.settings.aws_region.clone());
    let shared = aws_config::defaults(BehaviorVersion::latest())
        .region(region)
        .load()
        .await;
    let ctx = Ctx::new(settings, &shared);
    // `init` needs no Terraform and `doctor` reports on it itself.
    if !matches!(
        &cli.command,
        cli::Command::Init { .. } | cli::Command::Doctor
    ) {
        terraform::verify_version(&ctx.terraform).await?;
    }
    cli::dispatch(&ctx, &cli.command).await
}
