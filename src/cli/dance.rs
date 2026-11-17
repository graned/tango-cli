use anyhow::Result;
use clap::Args;
use serde_json::json;
use tracing::info;

use super::CommandContext;
use crate::core::image_ref::ImageRef;
use crate::core::tags::{Event, TagSet};
use crate::registry::{BuildOptions, DockerClient, RegistryClient};

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

    /// Buildx cache source (repeatable; replaces configured cache_from)
    #[arg(long)]
    pub cache_from: Vec<String>,

    /// Buildx cache destination (repeatable; replaces configured cache_to)
    #[arg(long)]
    pub cache_to: Vec<String>,

    /// For merge events: full source reference to promote from (tag or digest).
    /// If omitted, the sha-tag derived from --sha is used (e.g. sha-abc123def456).
    #[arg(long)]
    pub promote_from_sha: Option<String>,

    /// For merge events: force a fresh build instead of promoting the existing sha-tag.
    /// Use only if the image was never built for this SHA (e.g. hotfix directly on main).
    #[arg(long)]
    pub force_build: bool,
}

pub async fn run(ctx: CommandContext, args: DanceArgs) -> Result<()> {
    let cfg = &ctx.config;

    let image_cfg = cfg.image.as_ref().ok_or_else(|| {
        anyhow::anyhow!("No image config found. Specify --package or ensure image config is set.")
    })?;

    let tags = TagSet::generate(&args.event, &args.sha, args.pr, &cfg.tags);
    let base_ref = ImageRef::new(&cfg.registry.host, &cfg.registry.namespace, &image_cfg.name);

    let client = DockerClient::new(
        cfg.registry.username.clone(),
        std::env::var("TANGO_REGISTRY_PASSWORD").ok(),
    );

    // Parse build args
    let mut build_args: Vec<(String, String)> = args
        .build_args
        .iter()
        .filter_map(|a| {
            a.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
        })
        .collect();
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

    let cache_from = if args.cache_from.is_empty() {
        &image_cfg.cache_from
    } else {
        &args.cache_from
    };
    let cache_to = if args.cache_to.is_empty() {
        &image_cfg.cache_to
    } else {
        &args.cache_to
    };

    let digest: String;
    let primary_ref: ImageRef;

    match &args.event {
        Event::Merge => {
            let sha_tag = tags.sha_tag.as_deref().unwrap_or("latest");
            primary_ref = base_ref.clone().with_tag(sha_tag);

            if args.force_build {
                // Explicit opt-in: build fresh then promote (e.g. hotfix directly on main).
                info!(
                    "Merge event (--force-build): building {} then promoting to mutable tags",
                    primary_ref
                );
                digest = client
                    .push_image(
                        &primary_ref,
                        BuildOptions {
                            context: &image_cfg.context,
                            dockerfile: image_cfg.dockerfile.as_deref(),
                            build_args: &build_args,
                            labels: &labels,
                            platforms: &platforms,
                            cache_from,
                            cache_to,
                        },
                        ctx.dry_run,
                    )
                    .await?;
            } else {
                // Default: promote the already-built image — no rebuild.
                // Source is either an explicit --promote-from-sha ref or the sha-tag
                // derived from --sha (e.g. sha-abc123def456).
                let source: ImageRef = if let Some(src) = &args.promote_from_sha {
                    src.parse()?
                } else {
                    primary_ref.clone()
                };

                info!(
                    "Merge event: promoting {} → mutable tags (no rebuild)",
                    source
                );

                let mutable_tags: Vec<ImageRef> = tags
                    .all
                    .iter()
                    .filter(|t| Some(*t) != tags.sha_tag.as_ref())
                    .map(|t| base_ref.clone().with_tag(t))
                    .collect();

                if mutable_tags.is_empty() {
                    anyhow::bail!(
                        "No mutable tags to promote to. Check your tag config (main_tag, latest_tag)."
                    );
                }

                client
                    .tag_image(&source, &mutable_tags, ctx.dry_run)
                    .await?;
                digest = client
                    .get_digest(&primary_ref)
                    .await
                    .unwrap_or_else(|_| "sha256:unknown".into());
                return output_result(ctx, base_ref, tags, digest);
            }

            let mutable_refs: Vec<ImageRef> = tags
                .all
                .iter()
                .filter(|t| Some(*t) != tags.sha_tag.as_ref())
                .map(|t| base_ref.clone().with_tag(t))
                .collect();

            if !mutable_refs.is_empty() {
                client
                    .tag_image(&primary_ref, &mutable_refs, ctx.dry_run)
                    .await?;
            }
        }

        Event::Commit | Event::PullRequest => {
            // Publish: build with sha-tag and any PR/feature tags.
            let sha_tag = tags.sha_tag.as_deref().unwrap_or("latest");
            primary_ref = base_ref.clone().with_tag(sha_tag);

            info!(
                "Publish event {:?}: building and pushing {}",
                args.event, primary_ref
            );
            digest = client
                .push_image(
                    &primary_ref,
                    BuildOptions {
                        context: &image_cfg.context,
                        dockerfile: image_cfg.dockerfile.as_deref(),
                        build_args: &build_args,
                        labels: &labels,
                        platforms: &platforms,
                        cache_from,
                        cache_to,
                    },
                    ctx.dry_run,
                )
                .await?;

            let extra_refs: Vec<ImageRef> = tags
                .all
                .iter()
                .filter(|t| Some(*t) != tags.sha_tag.as_ref())
                .map(|t| base_ref.clone().with_tag(t))
                .collect();

            if !extra_refs.is_empty() {
                client
                    .tag_image(&primary_ref, &extra_refs, ctx.dry_run)
                    .await?;
            }
        }
    }

    output_result(ctx, base_ref, tags, digest)
}

fn output_result(
    ctx: CommandContext,
    base_ref: ImageRef,
    tags: crate::core::tags::TagSet,
    digest: String,
) -> Result<()> {
    let output = json!({
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
        println!("Image: {}@{}", base_ref.repository(), digest);
        println!("Tags:  {}", tags.all.join(", "));
    }

    Ok(())
}
