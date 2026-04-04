pub mod loader;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TangoConfig {
    pub registry: RegistryConfig,
    #[serde(default)]
    pub defaults: DefaultsConfig,
    #[serde(default)]
    pub packages: HashMap<String, PackageConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct RegistryConfig {
    pub host: String,
    pub namespace: String,
    /// Optional: registry username (prefer env TANGO_REGISTRY_USER)
    pub username: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct DefaultsConfig {
    #[serde(default)]
    pub tags: TagDefaults,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TagDefaults {
    #[serde(default = "default_sha_prefix")]
    pub sha_prefix: String,
    #[serde(default = "default_pr_prefix")]
    pub pr_prefix: String,
    #[serde(default = "default_main_tag")]
    pub main_tag: String,
    #[serde(default = "default_latest_tag")]
    pub latest_tag: String,
    #[serde(default = "default_true")]
    pub publish_latest_on_merge: bool,
}

impl Default for TagDefaults {
    fn default() -> Self {
        Self {
            sha_prefix: default_sha_prefix(),
            pr_prefix: default_pr_prefix(),
            main_tag: default_main_tag(),
            latest_tag: default_latest_tag(),
            publish_latest_on_merge: true,
        }
    }
}

fn default_sha_prefix() -> String {
    "sha-".into()
}
fn default_pr_prefix() -> String {
    "pr-".into()
}
fn default_main_tag() -> String {
    "main".into()
}
fn default_latest_tag() -> String {
    "latest".into()
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct PackageConfig {
    pub image: ImageConfig,
    /// Per-package registry override
    pub registry: Option<RegistryConfig>,
    /// Per-package tag defaults override
    pub defaults: Option<DefaultsConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct ImageConfig {
    pub name: String,
    pub context: String,
    pub dockerfile: Option<String>,
    #[serde(default)]
    pub build_args: HashMap<String, String>,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub platforms: Vec<String>,
}

/// Fully resolved config after merging root + package + env overrides
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub registry: RegistryConfig,
    pub tags: TagDefaults,
    pub image: Option<ImageConfig>,
    #[allow(dead_code)]
    pub package_name: Option<String>,
    pub raw: TangoConfig,
}
