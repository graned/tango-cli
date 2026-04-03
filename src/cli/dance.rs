use anyhow::Result;
use clap::Args;
use serde_json::json;
use tracing::info;

use crate::core::tags::{Event, TagSet};
use crate::core::image_ref::ImageRef;
use crate::registry::{DockerClient, RegistryClient};
use super::CommandContext;

/// DanceArgs combines what's needed for both publish and promote flows.
#[derive(Args, Debug)]
pub struct DanceArgs {
    /// CI event type: commit | pr | merge
    #[arg(long, env = "TANGO_EVENT")]
    pub event: Event,

    /// Git commit SHA
    #[arg(long, env = "TANGO_SHA", alias = "commit")]
    pub sha: String,

    /// Pull request number (required for pr event)
    #[arg(long, env = "TANGO_PR_NUMBER")]
    pub pr: Option<u64>,

    /// Extra build args passed to docker build (KEY=VALUE, repeatable)
    #[arg(long = "build-arg")]
    pub build_args: Vec<String>,

    /// Extra labels passed to docker build (KEY=VALUE, repeatable)
    #[arg(long = "label")]
    pub labels: Vec<String>,

    /// Target platforms (e.g. linux/amd64,linux/arm64)
    #[arg(long)]
    pub platform: Vec<String>,

    /// For merge events: the sha-tag of the already-built image to promote from.
    /// If not set, tango will build first, then promote.
    #[arg(long)]
    pub promote_from_sha: Option<String>,
}

pub async fn run(ctx: CommandContext, args: DanceArgs) -> Result<()> {
    let cfg = &ctx.config;

    let image_cfg = cfg.image.as_ref()
        .ok_or_else(|| anyhow::anyhow!(
            "No image config found. Specify --package or ensure image config is set."
        ))?;

    let tags = TagSet::generate(&args.event, &args.sha, args.pr, &cfg.tags);
    let base_ref = ImageRef::new(&cfg.registry.host, &cfg.registry.namespace, &image_cfg.name);

    let client = DockerClient::new(
        cfg.registry.username.clone(),
        std::env::var("TANGO_REGISTRY_PASSWORD").ok(),
    );

    // Parse build args
    let mut build_args: Vec<(String, String)> = args.build_args.iter()
        .filter_map(|a| a.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
        .collect();
    for (k, v) in &image_cfg.build_args {
        if !build_args.iter().any(|(ck, _)| ck == k) {
            build_args.push((k.clone(), v.clone()));
        }
    }

    let mut labels: Vec<(String, String)> = args.labels.iter()
        .filter_map(|l| l.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
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

    let digest: String;
    let primary_ref: ImageRef;

    match &args.event {
        Event::Merge => {
            // For merge: either promote from an existing sha-tag, or build fresh then promote.
            let sha_tag = tags.sha_tag.as_deref().unwrap_or("latest");
            primary_ref = base_ref.clone().with_tag(sha_tag);

            if let Some(promote_from) = &args.promote_from_sha {
                // Caller already has the sha-tagged image — just retag it.
                info!("Merge event: promoting existing image {} to mutable tags", promote_from);
                let source: ImageRef = promote_from.parse()?;

                let mutable_refs: Vec<ImageRef> = tags.all.iter()
                    .filter(|t| Some(*t) != tags.sha_tag.as_ref())
                    .map(|t| base_ref.clone().with_tag(t))
                    .collect();

                // Also tag to the sha-tag for traceability (idempotent if already exists).
                let all_targets: Vec<ImageRef> = std::iter::once(primary_ref.clone())
                    .chain(mutable_refs)
                    .collect();

                client.tag_image(&source, &all_targets, ctx.dry_run).await?;
                digest = client.get_digest(&primary_ref).await.unwrap_or_else(|_| "sha256:unknown".into());
            } else {
                // Build first with sha-tag, then retag to mutable tags.
                info!("Merge event: building and pushing {} then promoting to mutable tags", primary_ref);
                digest = client.push_image(
                    &primary_ref,
                    &image_cfg.context,
                    image_cfg.dockerfile.as_deref(),
                    &build_args,
                    &labels,
                    &platforms,
                    ctx.dry_run,
                ).await?;

                let mutable_refs: Vec<ImageRef> = tags.all.iter()
                    .filter(|t| Some(*t) != tags.sha_tag.as_ref())
                    .map(|t| base_ref.clone().with_tag(t))
                    .collect();

                if !mutable_refs.is_empty() {
                    client.tag_image(&primary_ref, &mutable_refs, ctx.dry_run).await?;
                }
            }
        }

        Event::Commit | Event::PullRequest => {
            // Publish: build with sha-tag and any PR/feature tags.
            let sha_tag = tags.sha_tag.as_deref().unwrap_or("latest");
            primary_ref = base_ref.clone().with_tag(sha_tag);

            info!("Publish event {:?}: building and pushing {}", args.event, primary_ref);
            digest = client.push_image(
                &primary_ref,
                &image_cfg.context,
                image_cfg.dockerfile.as_deref(),
                &build_args,
                &labels,
                &platforms,
                ctx.dry_run,
            ).await?;

            let extra_refs: Vec<ImageRef> = tags.all.iter()
                .filter(|t| Some(*t) != tags.sha_tag.as_ref())
                .map(|t| base_ref.clone().with_tag(t))
                .collect();

            if !extra_refs.is_empty() {
                client.tag_image(&primary_ref, &extra_refs, ctx.dry_run).await?;
            }
        }
    }

    let output = json!({
        "event": format!("{:?}", args.event).to_lowercase(),
        "image": base_ref.repository(),
        "sha_tag": tags.sha_tag,
        "pr_tag": tags.pr_tag,
        "main_tag": tags.main_tag,
        "latest_tag": tags.latest_tag,
        "tags": tags.all,
        "digest": digest,
        "reference": format!("{}@{}", base_ref.repository(), digest),
        "dry_run": ctx.dry_run,
    });

    if ctx.json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("Dance complete for event {:?}", args.event);
        println!("Image: {}@{}", base_ref.repository(), digest);
        println!("Tags:  {}", tags.all.join(", "));
    }

    Ok(())
}
