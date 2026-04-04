use anyhow::{bail, Result};
use clap::Args;
use serde_json::json;

use super::CommandContext;
use crate::core::{
    image_ref::ImageRef,
    tags::{Event, TagSet},
};

#[derive(Args, Debug)]
pub struct PrintRefArgs {
    /// Specific tag to print (overrides event/sha-derived tag)
    #[arg(long, conflicts_with_all = ["event", "sha"])]
    pub tag: Option<String>,

    /// Use a digest instead of a tag
    #[arg(long, conflicts_with = "tag")]
    pub digest: Option<String>,

    /// CI event type (used to derive tag when --tag is not set)
    #[arg(long, env = "TANGO_EVENT", requires = "sha")]
    pub event: Option<Event>,

    /// Git commit SHA (used to derive sha-tag)
    #[arg(long, env = "TANGO_SHA")]
    pub sha: Option<String>,

    /// Pull request number (required when event=pr)
    #[arg(long, env = "TANGO_PR_NUMBER")]
    pub pr: Option<u64>,

    /// Which tag from the generated set to print: sha|pr|main|latest (default: sha)
    #[arg(long, default_value = "sha")]
    pub select: String,

    /// Registry host override
    #[arg(long)]
    pub registry: Option<String>,

    /// Namespace override
    #[arg(long)]
    pub namespace: Option<String>,

    /// Image name override
    #[arg(long)]
    pub image: Option<String>,
}

pub async fn run(ctx: CommandContext, args: PrintRefArgs) -> Result<()> {
    let cfg = &ctx.config;

    let registry_host = args.registry.as_deref().unwrap_or(&cfg.registry.host);
    let namespace = args.namespace.as_deref().unwrap_or(&cfg.registry.namespace);
    let image_name = if let Some(n) = &args.image {
        n.clone()
    } else {
        cfg.image.as_ref().map(|i| i.name.clone()).ok_or_else(|| {
            anyhow::anyhow!("No image name configured. Specify --image or --package.")
        })?
    };

    let base_ref = ImageRef::new(registry_host, namespace, &image_name);

    let image_ref: ImageRef = if let Some(digest) = &args.digest {
        base_ref.with_digest(digest)
    } else if let Some(tag) = &args.tag {
        base_ref.with_tag(tag)
    } else if let (Some(event), Some(sha)) = (&args.event, &args.sha) {
        let tag_set = TagSet::generate(event, sha, args.pr, &cfg.tags);
        let selected_tag = match args.select.as_str() {
            "sha" => tag_set.sha_tag.as_deref(),
            "pr" => tag_set.pr_tag.as_deref(),
            "main" => tag_set.main_tag.as_deref(),
            "latest" => tag_set.latest_tag.as_deref(),
            other => bail!("Unknown --select value '{}': use sha|pr|main|latest", other),
        };
        let tag = selected_tag.ok_or_else(|| {
            anyhow::anyhow!(
                "Tag '{}' is not available for event '{:?}'",
                args.select,
                event
            )
        })?;
        base_ref.with_tag(tag)
    } else {
        bail!("Specify --tag, --digest, or both --event and --sha");
    };

    let output = json!({
        "reference": image_ref.to_string(),
        "registry": image_ref.registry,
        "namespace": image_ref.namespace,
        "name": image_ref.name,
        "tag": image_ref.tag,
        "digest": image_ref.digest,
    });

    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("{}", image_ref);
    }

    Ok(())
}
