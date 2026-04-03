use anyhow::{bail, Result};
use clap::Args;
use serde_json::json;

use crate::core::image_ref::ImageRef;
use crate::registry::{DockerClient, RegistryClient};
use super::CommandContext;

#[derive(Args, Debug)]
pub struct PromoteArgs {
    /// Source image reference (tag or digest). If omitted, derived from --sha using the
    /// configured sha_prefix.
    #[arg(long)]
    pub source: Option<String>,

    /// SHA used to derive the source sha-tag when --source is not specified.
    #[arg(long, env = "TANGO_SHA")]
    pub sha: Option<String>,

    /// Target tags to promote to (repeatable: --target-tag main --target-tag latest)
    #[arg(long = "target-tag", required = true)]
    pub target_tags: Vec<String>,

    /// Registry host override (defaults to config)
    #[arg(long)]
    pub registry: Option<String>,

    /// Namespace override (defaults to config)
    #[arg(long)]
    pub namespace: Option<String>,

    /// Image name override (defaults to config)
    #[arg(long)]
    pub image: Option<String>,
}

pub async fn run(ctx: CommandContext, args: PromoteArgs) -> Result<()> {
    let cfg = &ctx.config;

    let registry_host = args.registry.as_deref().unwrap_or(&cfg.registry.host);
    let namespace = args.namespace.as_deref().unwrap_or(&cfg.registry.namespace);
    let image_name = if let Some(n) = &args.image {
        n.clone()
    } else {
        cfg.image.as_ref()
            .map(|i| i.name.clone())
            .ok_or_else(|| anyhow::anyhow!(
                "No image name found. Specify --image, --package, or configure an image."
            ))?
    };

    // Resolve source reference
    let source_ref: ImageRef = if let Some(src) = &args.source {
        src.parse()?
    } else if let Some(sha) = &args.sha {
        let short_sha = &sha[..sha.len().min(12)];
        let sha_tag = format!("{}{}", cfg.tags.sha_prefix, short_sha);
        ImageRef::new(registry_host, namespace, &image_name).with_tag(sha_tag)
    } else {
        bail!("Either --source or --sha must be provided");
    };

    if args.target_tags.is_empty() {
        bail!("At least one --target-tag must be specified");
    }

    let base_ref = ImageRef::new(registry_host, namespace, &image_name);
    let target_refs: Vec<ImageRef> = args.target_tags.iter()
        .map(|t| base_ref.clone().with_tag(t))
        .collect();

    let client = DockerClient::new(
        cfg.registry.username.clone(),
        std::env::var("TANGO_REGISTRY_PASSWORD").ok(),
    );

    client.tag_image(&source_ref, &target_refs, ctx.dry_run).await?;

    let output = json!({
        "source": source_ref.to_string(),
        "targets": target_refs.iter().map(|r| r.to_string()).collect::<Vec<_>>(),
        "dry_run": ctx.dry_run,
    });

    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        for target in &target_refs {
            println!("Promoted: {} → {}", source_ref, target);
        }
    }

    Ok(())
}
