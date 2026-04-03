use anyhow::{bail, Result};
use clap::Args;
use serde_json::json;

use crate::core::image_ref::ImageRef;
use crate::registry::{DockerClient, RegistryClient};
use super::CommandContext;

#[derive(Args, Debug)]
pub struct ResolveArgs {
    /// Tag to resolve (e.g. main, sha-abc123)
    #[arg(long, conflicts_with = "image_ref")]
    pub tag: Option<String>,

    /// Full image reference to resolve (e.g. ghcr.io/org/app:main)
    #[arg(long = "ref", conflicts_with = "tag")]
    pub image_ref: Option<String>,

    /// Registry host override (used with --tag)
    #[arg(long)]
    pub registry: Option<String>,

    /// Namespace override (used with --tag)
    #[arg(long)]
    pub namespace: Option<String>,

    /// Image name override (used with --tag)
    #[arg(long)]
    pub image: Option<String>,
}

pub async fn run(ctx: CommandContext, args: ResolveArgs) -> Result<()> {
    let cfg = &ctx.config;

    let image_ref: ImageRef = if let Some(r) = &args.image_ref {
        r.parse()?
    } else if let Some(tag) = &args.tag {
        let registry_host = args.registry.as_deref().unwrap_or(&cfg.registry.host);
        let namespace = args.namespace.as_deref().unwrap_or(&cfg.registry.namespace);
        let image_name = if let Some(n) = &args.image {
            n.clone()
        } else {
            cfg.image.as_ref()
                .map(|i| i.name.clone())
                .ok_or_else(|| anyhow::anyhow!(
                    "No image name configured. Specify --image or --package."
                ))?
        };
        ImageRef::new(registry_host, namespace, &image_name).with_tag(tag)
    } else {
        bail!("Either --tag or --ref must be specified");
    };

    let client = DockerClient::new(
        cfg.registry.username.clone(),
        std::env::var("TANGO_REGISTRY_PASSWORD").ok(),
    );

    let digest = client.get_digest(&image_ref).await?;

    let output = json!({
        "reference": image_ref.to_string(),
        "digest": digest,
        "pinned": format!("{}@{}", image_ref.repository(), digest),
    });

    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("{}", digest);
    }

    Ok(())
}
