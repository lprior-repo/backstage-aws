use crate::{
    ctx::Ctx,
    error::{Error, Result},
    settings::Settings,
};
use clap::{Parser, Subcommand};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "backstage-aws",
    version,
    about = "AI-agent-first Backstage on AWS operator",
    long_about = "Provision and operate Backstage on AWS Fargate with Terraform and GitHub Actions OIDC. All options accept BACKSTAGE_AWS_* environment fallbacks. No command prompts or reads a TTY. Every command writes exactly one JSON result envelope to stdout (only --help and --version print plain text); progress and child output go to stderr. Exit codes: 0 success, 2 invalid input, 3 blocked or approval required, 4 tool/API failure, 5 remote work continues after timeout."
)]
pub(crate) struct Cli {
    #[command(flatten)]
    pub(crate) settings: Settings,
    #[command(subcommand)]
    pub(crate) command: Command,
}
#[derive(Subcommand, Clone, Debug)]
pub(crate) enum Command {
    #[command(
        about = "Render the embedded starter into a new directory.",
        long_about = "Creates a new project from the embedded template. Requires --output DIR; the destination must not exist."
    )]
    Init {
        #[arg(long)]
        output: PathBuf,
    },
    #[command(
        about = "Run offline Terraform format, initialization, validation, and tests for all stacks."
    )]
    Check,
    #[command(about = "Check local tools, AWS identity, and GitHub repository permissions.")]
    Doctor,
    #[command(about = "Provision account bootstrap resources and migrate bootstrap state.")]
    Bootstrap,
    #[command(about = "Provision shared base infrastructure and initialize the database role.")]
    Base,
    #[command(about = "Upload and restore a custom-format PostgreSQL seed dump.")]
    Seed {
        #[arg(long)]
        file: PathBuf,
    },
    #[command(about = "Build and push the portal image with registry cache.")]
    Build {
        #[arg(long)]
        tag: Option<String>,
    },
    #[command(subcommand)]
    Env(EnvCommand),
    #[command(subcommand)]
    Secrets(SecretsCommand),
    #[command(subcommand)]
    Db(DbCommand),
    #[command(about = "Create the GitHub repository if needed and publish the project to main.")]
    Publish,
    #[command(
        about = "Run doctor, bootstrap, base, optional seed and sign-in credentials, environment deployment, and publish in order."
    )]
    Up {
        #[arg(long)]
        seed_file: Option<PathBuf>,
        /// GitHub sign-in credentials, stored once the base stack exists.
        #[arg(long)]
        identity_file: Option<PathBuf>,
    },
}
#[derive(Subcommand, Clone, Debug)]
pub(crate) enum EnvCommand {
    #[command(about = "Clone the namespace database and deploy its Fargate service.")]
    Up {
        #[arg(long)]
        digest: Option<String>,
        #[arg(long)]
        scan: bool,
    },
    #[command(
        about = "Destroy namespace resources, drop its database, and delete its state object."
    )]
    Down,
    #[command(about = "List namespaces with Terraform state in the state bucket.")]
    List,
    #[command(about = "Report namespace service, image, URL, and readiness.")]
    Status,
}
#[derive(Subcommand, Clone, Debug)]
pub(crate) enum SecretsCommand {
    #[command(about = "Set GitHub OAuth identity credentials from a private file or environment.")]
    SetIdentity {
        #[arg(long)]
        file: Option<PathBuf>,
    },
}
#[derive(Subcommand, Clone, Debug)]
pub(crate) enum DbCommand {
    #[command(
        about = "Generate and apply a new PostgreSQL application-role password, then restart services.",
        long_about = "Requires --approve. The random password is handled only in Secrets Manager API bodies and is never printed, logged, or passed as an argument."
    )]
    RotatePassword,
}
fn value<T: Serialize>(result: T) -> Result<Value> {
    serde_json::to_value(result).map_err(|error| Error::state_failed(error.to_string()))
}
pub(crate) const fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Init { .. } => "init",
        Command::Check => "check",
        Command::Doctor => "doctor",
        Command::Bootstrap => "bootstrap",
        Command::Base => "base",
        Command::Seed { .. } => "seed",
        Command::Build { .. } => "build",
        Command::Env(EnvCommand::Up { .. }) => "env up",
        Command::Env(EnvCommand::Down) => "env down",
        Command::Env(EnvCommand::List) => "env list",
        Command::Env(EnvCommand::Status) => "env status",
        Command::Secrets(SecretsCommand::SetIdentity { .. }) => "secrets set-identity",
        Command::Db(DbCommand::RotatePassword) => "db rotate-password",
        Command::Publish => "publish",
        Command::Up { .. } => "up",
    }
}
pub(crate) async fn dispatch(ctx: &Ctx, command: &Command) -> Result<Value> {
    use crate::ops;
    match command {
        Command::Init { output } => value(crate::template::render_template(&ctx.settings, output)?),
        Command::Check => value(crate::check::check(&ctx.project_dir, &ctx.terraform).await?),
        Command::Doctor => value(ops::doctor(ctx).await?),
        Command::Bootstrap => value(ops::up::bootstrap(ctx).await?),
        Command::Base => value(ops::up::base(ctx).await?),
        Command::Seed { file } => value(crate::db::seed(ctx, file).await?),
        Command::Build { tag } => value(ops::build(ctx, tag.as_deref()).await?),
        Command::Env(EnvCommand::Up { digest, scan }) => {
            value(Box::pin(ops::env_up(ctx, digest.as_deref(), *scan)).await?)
        }
        Command::Env(EnvCommand::Down) => value(ops::env_down(ctx).await?),
        Command::Env(EnvCommand::List) => value(ops::env_list(ctx).await?),
        Command::Env(EnvCommand::Status) => value(ops::env_status(ctx).await?),
        Command::Secrets(SecretsCommand::SetIdentity { file }) => {
            value(ops::set_identity(ctx, file.as_deref()).await?)
        }
        Command::Db(DbCommand::RotatePassword) => value(Box::pin(ops::rotate_password(ctx)).await?),
        Command::Publish => value(ops::publish(ctx).await?),
        Command::Up {
            seed_file,
            identity_file,
        } => {
            let up = ops::up(ctx, seed_file.as_deref(), identity_file.as_deref());
            value(Box::pin(up).await?)
        }
    }
}
