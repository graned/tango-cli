pub mod client;

use crate::core::image_ref::ImageRef;
use anyhow::Result;
use async_trait::async_trait;

/// Options passed to `push_image` — grouped to stay within argument-count limits.
pub struct BuildOptions<'a> {
    pub context: &'a str,
    pub dockerfile: Option<&'a str>,
    pub build_args: &'a [(String, String)],
    pub labels: &'a [(String, String)],
    pub platforms: &'a [String],
    pub cache_from: &'a [String],
    pub cache_to: &'a [String],
}

#[async_trait]
pub trait RegistryClient: Send + Sync {
    /// Build and push an image, returning its digest.
    async fn push_image(
        &self,
        image_ref: &ImageRef,
        opts: BuildOptions<'_>,
        dry_run: bool,
    ) -> Result<String>;

    /// Retag an existing image (by source tag or digest) to new tags — no rebuild
    async fn tag_image(&self, source: &ImageRef, targets: &[ImageRef], dry_run: bool)
        -> Result<()>;

    /// Resolve a tag to its digest
    async fn get_digest(&self, image_ref: &ImageRef) -> Result<String>;

    /// Check whether an image reference exists in the registry
    #[allow(dead_code)]
    async fn check_exists(&self, image_ref: &ImageRef) -> Result<bool>;
}

pub use client::DockerClient;
