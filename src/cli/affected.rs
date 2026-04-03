use anyhow::{Context, Result};
use clap::Args;
use serde_json::json;
use std::collections::HashSet;
use tokio::process::Command;
use tracing::debug;

use super::CommandContext;

#[derive(Args, Debug)]
pub struct AffectedArgs {
    /// Base ref for the diff (e.g. origin/main, a commit SHA)
    #[arg(long, default_value = "origin/main")]
    pub base: String,

    /// Head ref for the diff (default: HEAD)
    #[arg(long, default_value = "HEAD")]
    pub head: String,

    /// Also include packages that depend on changed packages (transitive)
    #[arg(long)]
    pub with_dependents: bool,
}

pub async fn run(ctx: CommandContext, args: AffectedArgs) -> Result<()> {
    let cfg = &ctx.config;

    // Run git diff to get changed files
    debug!("Running: git diff --name-only {}..{}", args.base, args.head);
    let output = Command::new("git")
        .args(["diff", "--name-only", &format!("{}..{}", args.base, args.head)])
        .output()
        .await
        .context("Failed to run git diff — is this a git repository?")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git diff failed: {}", stderr.trim());
    }

    let changed_files: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.to_string())
        .collect();

    debug!("Changed files: {:?}", changed_files);

    // Map changed files to affected packages based on context paths
    let mut affected: HashSet<String> = HashSet::new();

    for (pkg_name, pkg_cfg) in &cfg.raw.packages {
        let context = &pkg_cfg.image.context;
        // Normalize context path: strip leading "./"
        let normalized_context = context.trim_start_matches("./");

        for file in &changed_files {
            let normalized_file = file.trim_start_matches("./");
            if normalized_file.starts_with(normalized_context)
                || normalized_context == "."
                || normalized_context.is_empty()
            {
                affected.insert(pkg_name.clone());
                break;
            }
        }
    }

    // Sort for deterministic output
    let mut affected_list: Vec<String> = affected.into_iter().collect();
    affected_list.sort();

    let output_data = json!({
        "base": args.base,
        "head": args.head,
        "changed_files": changed_files,
        "affected_packages": affected_list,
    });

    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&output_data)?);
    } else if affected_list.is_empty() {
        println!("No affected packages detected.");
    } else {
        println!("Affected packages:");
        for pkg in &affected_list {
            println!("  - {}", pkg);
        }
    }

    Ok(())
}
