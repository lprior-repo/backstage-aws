use crate::{
    ctx::Ctx,
    error::{Error, Result},
    settings::Settings,
};
use clap::{Parser, Subcommand};
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;

/// Parse CLI flag combinations once; the execution core accepts semantic modes.
pub(crate) fn check_request<'a>(
    app: bool,
    runtime: bool,
    postgres_bin: Option<&'a std::path::Path>,
    release_archive: Option<&'a std::path::Path>,
) -> Result<crate::check::CheckRequest<'a>> {
    use crate::check::{ApplicationCheck, CheckRequest};
    let application = match (app, runtime, postgres_bin) {
        (_, true, postgres_bin) => ApplicationCheck::Runtime { postgres_bin },
        (_, false, Some(_)) => {
            return Err(Error::invalid_input("--postgres-bin requires --runtime"));
        }
        (true, false, None) => ApplicationCheck::Build,
        (false, false, None) => ApplicationCheck::None,
    };
    match (application, release_archive) {
        (ApplicationCheck::None, Some(archive)) => Ok(CheckRequest::Release { archive }),
        (application, release_archive) => Ok(CheckRequest::Project {
            application,
            release_archive,
        }),
    }
}

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
        /// Rehearse PostgreSQL 16 backup/restore and the running app (implies --app).
        #[arg(long)]
        runtime: bool,
        /// Directory containing PostgreSQL 16 tools, instead of searching PATH.
        #[arg(long, requires = "runtime")]
        postgres_bin: Option<PathBuf>,
        /// Smoke-test a local release archive; does not verify its provenance.
        #[arg(long)]
        release_archive: Option<PathBuf>,
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
    #[command(
        subcommand,
        about = "Add, remove and list Backstage plugins without hand-editing the app."
    )]
    Plugin(PluginCommand),
    #[command(
        subcommand,
        about = "Read or change app config YAML without losing comments."
    )]
    Config(ConfigCommand),
    #[command(
        about = "Run Backstage on this machine: local PostgreSQL, dev config and `yarn start`, no AWS.",
        long_about = "Starts PostgreSQL 16 in Docker Compose with a persistent project-local volume, writes app-config.local.yaml from the example if missing, installs dependencies if needed, and runs the dev server at http://localhost:3000 until Ctrl-C. Needs Node 24.22.1 and Docker Engine with Compose. Stops and removes its containers on exit, preserving database data unless --reset is used. Prints one JSON document when it exits."
    )]
    Dev(crate::dev::DevArgs),
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
#[derive(Subcommand, Clone, Debug)]
pub(crate) enum PluginCommand {
    #[command(about = "List the plugins you can add by name and which are installed. Read-only.")]
    List,
    #[command(
        about = "Add plugins to the app: dependencies, backend registration, config and code, in one step.",
        long_about = "Edits the project's app/ directory only; nothing is deployed. Name catalog plugins (see `plugin list`) and/or pass any package with --frontend / --backend. Versions come from the app's pinned Backstage release unless given as PKG@VERSION. Every change is computed first and written together, so a refusal leaves the files untouched. Safe to repeat. --install then updates yarn.lock."
    )]
    Add(crate::plugin::EditArgs),
    #[command(
        about = "Remove plugins added by `plugin add`: dependencies, backend registration, config and code.",
        long_about = "The inverse of `plugin add`, with the same arguments. Safe to repeat."
    )]
    Remove(crate::plugin::EditArgs),
}
#[derive(Subcommand, Clone, Debug)]
pub(crate) enum ConfigCommand {
    #[command(about = "Print the value at a dotted path in an app config YAML file. Read-only.")]
    Get {
        /// Dotted path such as `catalog.providers.github`.
        path: String,
        /// Config file inside app/, default app-config.yaml.
        #[arg(long, default_value = "app-config.yaml")]
        file: String,
    },
    #[command(
        about = "Set a scalar at a dotted path in an app config YAML file, keeping comments and layout.",
        long_about = "Creates missing parents. The value is read as a boolean, integer or text (true, 42, hello). Use ${VAR} to read an environment variable at runtime; never write secrets into the file."
    )]
    Set {
        /// Dotted path such as `backend.cors.origin`.
        path: String,
        /// The new value.
        value: String,
        /// Config file inside app/, default app-config.yaml.
        #[arg(long, default_value = "app-config.yaml")]
        file: String,
    },
}

impl Command {
    /// Commands that only touch the local project: no settings, AWS or Terraform.
    pub(crate) const fn is_project_local(&self) -> bool {
        matches!(self, Self::Plugin(_) | Self::Config(_) | Self::Dev(_))
    }
}

/// Runs a project-local command against `project`'s `app/` directory.
pub(crate) async fn project_command(settings: &Settings, command: &Command) -> Result<Value> {
    let project = &settings.project;
    match command {
        Command::Plugin(PluginCommand::List) => crate::plugin::list(project),
        Command::Plugin(PluginCommand::Add(args)) => crate::plugin::add(project, args).await,
        Command::Plugin(PluginCommand::Remove(args)) => crate::plugin::remove(project, args).await,
        Command::Config(ConfigCommand::Get { path, file }) => {
            crate::app_config::get(&project.join("app"), file, path)
        }
        Command::Config(ConfigCommand::Set { path, value, file }) => {
            crate::app_config::set(&project.join("app"), file, path, value)
        }
        Command::Dev(args) => crate::dev::run(project, settings.container_engine, args).await,
        _ => Err(Error::state_failed("not a project-local command")),
    }
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
        Command::Plugin(PluginCommand::List) => "plugin list",
        Command::Plugin(PluginCommand::Add(_)) => "plugin add",
        Command::Plugin(PluginCommand::Remove(_)) => "plugin remove",
        Command::Config(ConfigCommand::Get { .. }) => "config get",
        Command::Config(ConfigCommand::Set { .. }) => "config set",
        Command::Dev(_) => "dev",
        Command::Up { .. } => "up",
    }
}
pub(crate) async fn dispatch(ctx: &Ctx, command: &Command) -> Result<Value> {
    use crate::ops;
    match command {
        Command::Setup(args) => crate::onboarding::run(&ctx.settings.settings, args).await,
        Command::Init { output } => value(crate::template::render_template(&ctx.settings, output)?),
        Command::Check {
            app,
            runtime,
            postgres_bin,
            release_archive,
        } => value(
            crate::check::check(
                &ctx.project_dir,
                &ctx.terraform,
                check_request(
                    *app,
                    *runtime,
                    postgres_bin.as_deref(),
                    release_archive.as_deref(),
                )?,
            )
            .await?,
        ),
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
        Command::Plugin(_) | Command::Config(_) | Command::Dev(_) => {
            project_command(&ctx.settings.settings, command).await
        }
        Command::Up {
            seed_file,
            identity_file,
        } => {
            let up = ops::up(ctx, seed_file.as_deref(), identity_file.as_deref());
            value(Box::pin(up).await?)
        }
    }
}
