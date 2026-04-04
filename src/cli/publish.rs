use anyhow::Result;
use clap::Args;
use serde_json::json;

use super::CommandContext;
use crate::core::{
    image_ref::ImageRef,
    tags::{Event, TagSet},
};
use crate::registry::{BuildOptions, DockerClient, RegistryClient};

#[derive(Args, Debug)]
pub struct PublishArgs {
    /// CI event type: commit | pr | merge
    #[arg(long, env = "TANGO_EVENT")]
    pub event: Event,

    /// Git commit SHA
    #[arg(long, env = "TANGO_SHA", alias = "commit")]
    pub sha: String,

    /// Pull request number (required for pr event)
    #[arg(long, env = "TANGO_PR_NUMBER")]
    pub pr: Option<u64>,

    /// Extra build args (KEY=VALUE)
    #[arg(long = "build-arg")]
    pub build_args: Vec<String>,

    /// Extra labels (KEY=VALUE)
    #[arg(long = "label")]
    pub labels: Vec<String>,

    /// Target platforms (e.g. linux/amd64,linux/arm64)
    #[arg(long)]
    pub platform: Vec<String>,
}

pub async fn run(ctx: CommandContext, args: PublishArgs) -> Result<()> {
    let cfg = &ctx.config;
    let image_cfg = cfg.image.as_ref().ok_or_else(|| {
        anyhow::anyhow!("No image config found. Specify --package or ensure image config is set.")
    })?;

    let tags = TagSet::generate(&args.event, &args.sha, args.pr, &cfg.tags);

    let base_ref = ImageRef::new(&cfg.registry.host, &cfg.registry.namespace, &image_cfg.name);

    let primary_ref = base_ref
        .clone()
        .with_tag(tags.sha_tag.as_deref().unwrap_or("latest"));

    let client = DockerClient::new(
        cfg.registry.username.clone(),
        std::env::var("TANGO_REGISTRY_PASSWORD").ok(),
    );

    // Parse extra build args from CLI
    let mut build_args: Vec<(String, String)> = args
        .build_args
        .iter()
        .filter_map(|a| {
            a.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect();
    // Merge image-config build args (CLI overrides config)
    for (k, v) in &image_cfg.build_args {
        if !build_args.iter().any(|(ck, _)| ck == k) {
            build_args.push((k.clone(), v.clone()));
        }
    }

    let mut labels: Vec<(String, String)> = args
        .labels
        .iter()
        .filter_map(|l| {
            l.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect();
    for (k, v) in &image_cfg.labels {
        if !labels.iter().any(|(ck, _)| ck == k) {
            labels.push((k.clone(), v.clone()));
        }
    }

    let platforms: Vec<String> = if args.platform.is_empty() {
        image_cfg.platforms.clone()
    } else {
        args.platform.clone()
    };

    let dockerfile = image_cfg.dockerfile.as_deref();
    let digest = client
        .push_image(
            &primary_ref,
            BuildOptions {
                context: &image_cfg.context,
                dockerfile,
                build_args: &build_args,
                labels: &labels,
                platforms: &platforms,
            },
            ctx.dry_run,
        )
        .await?;

    // Apply additional mutable tags
    let additional_tags: Vec<ImageRef> = tags
        .all
        .iter()
        .filter(|t| Some(*t) != tags.sha_tag.as_ref())
        .map(|t| base_ref.clone().with_tag(t))
        .collect();

    if !additional_tags.is_empty() {
        client
            .tag_image(&primary_ref, &additional_tags, ctx.dry_run)
            .await?;
    }

    let output = json!({
        "image": base_ref.repository(),
        "sha_tag": tags.sha_tag,
        "pr_tag": tags.pr_tag,
        "main_tag": tags.main_tag,
        "latest_tag": tags.latest_tag,
        "tags": tags.all,
        "digest": digest,
        "reference": format!("{}@{}", base_ref.repository(), digest),
    });

    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Published: {}@{}", base_ref.repository(), digest);
        println!("Tags: {}", tags.all.join(", "));
    }

    Ok(())
}
