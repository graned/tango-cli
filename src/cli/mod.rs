pub mod affected;
pub mod dance;
pub mod print_ref;
pub mod promote;
pub mod publish;
pub mod resolve;

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::config;

#[derive(Parser, Debug)]
#[command(name = "tango", about = "Registry-agnostic artifact orchestration CLI", version)]
pub struct Cli {
    /// Path to config file (default: tango.yaml)
    #[arg(long, short = 'c', global = true, env = "TANGO_CONFIG")]
    pub config: Option<String>,

    /// Target package (for monorepos)
    #[arg(long, short = 'p', global = true, env = "TANGO_PACKAGE")]
    pub package: Option<String>,

    /// Dry run — print actions without executing
    #[arg(long, global = true, env = "TANGO_DRY_RUN")]
    pub dry_run: bool,

    /// Output as JSON
    #[arg(long, global = true, env = "TANGO_JSON")]
    pub json: bool,

    /// Enable verbose logging
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Build and push image with commit/PR tags
    Publish(publish::PublishArgs),
    /// Promote an existing image to new tags without rebuilding
    Promote(promote::PromoteArgs),
    /// Resolve a tag to its digest
    Resolve(resolve::ResolveArgs),
    /// Print the full image reference for a given tag
    PrintRef(print_ref::PrintRefArgs),
    /// Detect affected packages in a monorepo
    Affected(affected::AffectedArgs),
    /// Orchestrate the full build-tag-promote pipeline
    Dance(dance::DanceArgs),
}

pub async fn dispatch(cli: Cli) -> Result<()> {
    let cfg = config::loader::load(cli.config.as_deref(), cli.package.as_deref())?;
    let ctx = CommandContext {
        config: cfg,
        dry_run: cli.dry_run,
        json: cli.json,
        package: cli.package,
    };
    match cli.command {
        Commands::Publish(args) => publish::run(ctx, args).await,
        Commands::Promote(args) => promote::run(ctx, args).await,
        Commands::Resolve(args) => resolve::run(ctx, args).await,
        Commands::PrintRef(args) => print_ref::run(ctx, args).await,
        Commands::Affected(args) => affected::run(ctx, args).await,
        Commands::Dance(args) => dance::run(ctx, args).await,
    }
}

pub struct CommandContext {
    pub config: crate::config::ResolvedConfig,
    pub dry_run: bool,
    pub json: bool,
    #[allow(dead_code)]
    pub package: Option<String>,
}
