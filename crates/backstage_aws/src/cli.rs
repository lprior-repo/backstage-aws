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
    about = "Backstage on AWS Fargate: shared infrastructure once, an environment per pull request",
    long_about = "Runs Backstage on AWS Fargate with Terraform, and gives every pull request its own environment. Settings come from flags or BACKSTAGE_AWS_* environment variables; a project keeps them in environments/<environment>.env. No command prompts. Every command writes exactly one JSON document to stdout (only --help and --version print plain text); progress and child output go to stderr. Commands that change anything need --approve. Exit codes: 0 success, 2 invalid input, 3 blocked or approval required, 4 tool or API failure, 5 remote work continues after timeout."
)]
pub(crate) struct Cli {
    #[command(flatten)]
    pub(crate) settings: Settings,
    #[command(subcommand)]
    pub(crate) command: Command,
}
#[derive(Subcommand, Clone, Debug)]
pub(crate) enum Command {
    #[command(about = "Discover account prerequisites and show the next setup step. Read-only.")]
    Setup(crate::onboarding::SetupArgs),
    #[command(
        about = "Create a new project directory from the built-in template.",
        long_about = "Creates a new project from the built-in template and writes its settings to environments/<environment>.env. Requires --output DIR; the destination must not exist."
    )]
    Init {
        /// The directory to create.
        #[arg(long)]
        output: PathBuf,
    },
    #[command(
        about = "Validate Terraform offline, optionally including the Backstage application. Needs no AWS credentials."
    )]
    Check {
        /// Also install, typecheck, test, and build the Backstage application.
        #[arg(long)]
        app: bool,
    },
    #[command(about = "Check local tools, the AWS identity, and GitHub repository access.")]
    Doctor,
    #[command(
        about = "Prepare the AWS account: Terraform state, the CI deploy role, and the account baseline.",
        long_about = "Requires --approve. Creates the encrypted state bucket, the role CI assumes through GitHub OIDC, and (unless --account-baseline false) CloudTrail, a password policy and the S3 public-access block. Rewrites environments/<environment>.env. Safe to repeat."
    )]
    Bootstrap,
    #[command(
        about = "Create the shared infrastructure: network, load balancer, database, registry, secrets.",
        long_about = "Requires --approve and --allowed-ingress-cidrs. Everything environments share is created here once per account. Safe to repeat; rerun it after changing a setting."
    )]
    Base,
    #[command(about = "Load a database dump as the template every new environment is copied from.")]
    Seed {
        /// A custom-format dump: `pg_dump --format=custom`, or the output of `db dump --file`.
        #[arg(long)]
        file: PathBuf,
    },
    #[command(about = "Build the portal image and push it to the registry.")]
    Build {
        /// The image tag. Default: the namespace and the current time.
        #[arg(long)]
        tag: Option<String>,
    },
    #[command(subcommand, about = "Create, remove, list and inspect environments.")]
    Env(EnvCommand),
    #[command(subcommand, about = "Store secrets the portal reads.")]
    Secrets(SecretsCommand),
    #[command(subcommand, about = "Back up the database and rotate its password.")]
    Db(DbCommand),
    #[command(about = "Create the GitHub repository if needed and push the project to main.")]
    Publish,
    #[command(
        about = "First install in one command: doctor, bootstrap, base, seed, sign-in, environment, publish."
    )]
    Up {
        /// A database dump to load as the template for new environments.
        #[arg(long)]
        seed_file: Option<PathBuf>,
        /// GitHub sign-in credentials, stored once the base stack exists.
        #[arg(long)]
        identity_file: Option<PathBuf>,
    },
}
#[derive(Subcommand, Clone, Debug)]
pub(crate) enum EnvCommand {
    #[command(
        about = "Deploy the namespace and wait until it answers.",
        long_about = "Requires --approve. Builds the image (unless --digest names one), copies the template database for a new namespace, deploys, and returns once the load balancer reports a healthy target. With --pull-request and GITHUB_TOKEN it writes the environment's address on the pull request. Safe to repeat."
    )]
    Up {
        /// Deploy this existing image instead of building one.
        #[arg(long)]
        digest: Option<String>,
        /// Refuse the image when its vulnerability scan exceeds the finding limits.
        #[arg(long)]
        scan: bool,
    },
    #[command(
        about = "Remove the namespace: its service, its database, and its Terraform state.",
        long_about = "Requires --approve. Safe to repeat: an interrupted removal is finished by running it again."
    )]
    Down,
    #[command(about = "List the namespaces that exist in this account.")]
    List,
    #[command(about = "Report the namespace's address, image, and whether it is ready.")]
    Status,
}
#[derive(Subcommand, Clone, Debug)]
pub(crate) enum SecretsCommand {
    #[command(
        about = "Store the GitHub OAuth app credentials used for sign-in.",
        long_about = "Requires --approve. Reads clientId and clientSecret from --file (JSON), or from BACKSTAGE_AWS_OAUTH_CLIENT_ID and BACKSTAGE_AWS_OAUTH_CLIENT_SECRET. The values are never printed."
    )]
    SetIdentity {
        /// A JSON file: {"clientId": "...", "clientSecret": "..."}.
        #[arg(long)]
        file: Option<PathBuf>,
    },
}
#[derive(Subcommand, Clone, Debug)]
pub(crate) enum DbCommand {
    #[command(
        about = "Replace the database password the portal uses, then restart every environment.",
        long_about = "Requires --approve. The random password is handled only in Secrets Manager API bodies and is never printed, logged, or passed as an argument."
    )]
    RotatePassword,
    #[command(
        about = "Dump the namespace's database to the seed bucket, and optionally download it.",
        long_about = "Requires --approve. Runs pg_dump inside the VPC and stores the custom-format dump under backups/<namespace>/ in the seed bucket. --file also downloads it to a new file that only its owner can read; an existing file is never overwritten. `seed --file` restores such a dump as the template for new environments."
    )]
    Dump {
        #[arg(long)]
        file: Option<PathBuf>,
    },
}

#[cfg(test)]
#[path = "cli/tests.rs"]
mod tests;
fn value<T: Serialize>(result: T) -> Result<Value> {
    serde_json::to_value(result).map_err(|error| Error::state_failed(error.to_string()))
}
pub(crate) const fn command_name(command: &Command) -> &'static str {
    match command {
        Command::Setup(_) => "setup",
        Command::Init { .. } => "init",
        Command::Check { .. } => "check",
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
        Command::Db(DbCommand::Dump { .. }) => "db dump",
        Command::Publish => "publish",
        Command::Up { .. } => "up",
    }
}
pub(crate) async fn dispatch(ctx: &Ctx, command: &Command) -> Result<Value> {
    use crate::ops;
    match command {
        Command::Setup(args) => crate::onboarding::run(&ctx.settings.settings, args).await,
        Command::Init { output } => value(crate::template::render_template(&ctx.settings, output)?),
        Command::Check { app } => {
            value(crate::check::check(&ctx.project_dir, &ctx.terraform, *app).await?)
        }
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
        Command::Db(DbCommand::Dump { file }) => value(ops::dump(ctx, file.as_deref()).await?),
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
