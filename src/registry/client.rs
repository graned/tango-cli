use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use tokio::process::Command;
use tracing::{debug, info, warn};

use super::RegistryClient;
use crate::core::image_ref::ImageRef;

/// A registry client that delegates to the local `docker` CLI.
/// Works with any registry (Docker Hub, ECR, GCR, GHCR, etc.) because
/// authentication is handled by `docker login` or credential helpers.
pub struct DockerClient {
    pub username: Option<String>,
    pub password: Option<String>,
}

impl DockerClient {
    pub fn new(username: Option<String>, password: Option<String>) -> Self {
        Self { username, password }
    }

    /// Attempt `docker login` if credentials are provided.
    async fn maybe_login(&self, registry: &str) -> Result<()> {
        let (Some(user), Some(pass)) = (self.username.as_deref(), self.password.as_deref()) else {
            return Ok(());
        };

        info!("Logging in to registry {} as {}", registry, user);

        // Pipe the password via stdin to avoid it appearing in process arguments.
        let mut child = Command::new("docker")
            .args(["login", registry, "-u", user, "--password-stdin"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .context("Failed to spawn docker login")?;

        if let Some(stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let mut stdin = stdin;
            stdin
                .write_all(pass.as_bytes())
                .await
                .context("Failed to write password to docker login stdin")?;
        }

        let status = child.wait().await.context("docker login failed")?;
        if !status.success() {
            bail!("docker login to {} failed", registry);
        }
        Ok(())
    }

    /// Run a docker command and return (stdout, stderr, success).
    async fn run_docker(args: &[&str]) -> Result<(String, String, bool)> {
        debug!("docker {}", args.join(" "));
        let output = Command::new("docker")
            .args(args)
            .output()
            .await
            .with_context(|| format!("Failed to run: docker {}", args.join(" ")))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Ok((stdout, stderr, output.status.success()))
    }

    /// Extract a digest from `docker buildx imagetools inspect --format '{{json .Manifest}}'` output.
    async fn inspect_digest(image_ref: &str) -> Result<String> {
        let format_arg = "{{json .Manifest}}";
        let (stdout, stderr, ok) = Self::run_docker(&[
            "buildx",
            "imagetools",
            "inspect",
            "--format",
            format_arg,
            image_ref,
        ])
        .await?;

        if !ok {
            bail!(
                "docker buildx imagetools inspect failed for {}: {}",
                image_ref,
                stderr.trim()
            );
        }

        // The JSON contains a "digest" field at the top level
        let v: serde_json::Value = serde_json::from_str(stdout.trim())
            .with_context(|| format!("Failed to parse imagetools inspect output: {}", stdout))?;

        v["digest"].as_str().map(|s| s.to_string()).ok_or_else(|| {
            anyhow::anyhow!("No 'digest' field in imagetools inspect output: {}", stdout)
        })
    }
}

#[async_trait]
impl RegistryClient for DockerClient {
    async fn push_image(
        &self,
        image_ref: &ImageRef,
        context: &str,
        dockerfile: Option<&str>,
        build_args: &[(String, String)],
        labels: &[(String, String)],
        platforms: &[String],
        dry_run: bool,
    ) -> Result<String> {
        self.maybe_login(&image_ref.registry).await?;

        let ref_str = image_ref.to_string();
        let mut cmd_args: Vec<String> = vec![
            "buildx".into(),
            "build".into(),
            "--push".into(),
            "-t".into(),
            ref_str.clone(),
        ];

        if !platforms.is_empty() {
            cmd_args.push("--platform".into());
            cmd_args.push(platforms.join(","));
        }

        for (k, v) in build_args {
            cmd_args.push("--build-arg".into());
            cmd_args.push(format!("{}={}", k, v));
        }

        for (k, v) in labels {
            cmd_args.push("--label".into());
            cmd_args.push(format!("{}={}", k, v));
        }

        if let Some(df) = dockerfile {
            cmd_args.push("-f".into());
            cmd_args.push(df.to_string());
        }

        cmd_args.push(context.to_string());

        let args_ref: Vec<&str> = cmd_args.iter().map(|s| s.as_str()).collect();

        if dry_run {
            info!("[dry-run] docker {}", cmd_args.join(" "));
            return Ok("sha256:dry-run-digest-not-real-000000000000000000000000000000".into());
        }

        info!("Building and pushing: {}", ref_str);
        let (stdout, stderr, ok) = Self::run_docker(&args_ref).await?;
        debug!("build stdout: {}", stdout);
        if !stderr.is_empty() {
            debug!("build stderr: {}", stderr);
        }
        if !ok {
            bail!(
                "docker buildx build failed for {}: {}",
                ref_str,
                stderr.trim()
            );
        }

        // Retrieve digest after push
        let digest = Self::inspect_digest(&ref_str).await.unwrap_or_else(|e| {
            warn!(
                "Could not retrieve digest after push ({}), using placeholder",
                e
            );
            "sha256:unknown".to_string()
        });

        Ok(digest)
    }

    async fn tag_image(
        &self,
        source: &ImageRef,
        targets: &[ImageRef],
        dry_run: bool,
    ) -> Result<()> {
        let source_str = source.to_string();

        for target in targets {
            let target_str = target.to_string();

            if dry_run {
                info!(
                    "[dry-run] docker buildx imagetools create --tag {} {}",
                    target_str, source_str
                );
                continue;
            }

            info!("Retagging {} → {}", source_str, target_str);
            let (_, stderr, ok) = Self::run_docker(&[
                "buildx",
                "imagetools",
                "create",
                "--tag",
                &target_str,
                &source_str,
            ])
            .await?;

            if !ok {
                bail!(
                    "docker buildx imagetools create failed ({} → {}): {}",
                    source_str,
                    target_str,
                    stderr.trim()
                );
            }
        }

        Ok(())
    }

    async fn get_digest(&self, image_ref: &ImageRef) -> Result<String> {
        let ref_str = image_ref.to_string();
        Self::inspect_digest(&ref_str).await
    }

    async fn check_exists(&self, image_ref: &ImageRef) -> Result<bool> {
        let ref_str = image_ref.to_string();
        debug!("Checking existence of {}", ref_str);
        let (_, stderr, ok) = Self::run_docker(&["manifest", "inspect", &ref_str]).await?;
        if !ok {
            debug!(
                "manifest inspect returned non-zero for {}: {}",
                ref_str,
                stderr.trim()
            );
        }
        Ok(ok)
    }
}
