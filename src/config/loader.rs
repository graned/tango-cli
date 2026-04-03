use super::{ResolvedConfig, TangoConfig};
use anyhow::{Context, Result};
use std::path::Path;

pub fn load(config_path: Option<&str>, package: Option<&str>) -> Result<ResolvedConfig> {
    let path = config_path.unwrap_or("tango.yaml");
    let raw: TangoConfig = if Path::new(path).exists() {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read config file: {}", path))?;
        serde_yaml::from_str(&text)
            .with_context(|| format!("Failed to parse config file: {}", path))?
    } else {
        tracing::warn!("Config file '{}' not found, using defaults", path);
        TangoConfig::default()
    };

    // Apply env overrides to registry
    let mut registry = raw.registry.clone();
    if let Ok(host) = std::env::var("TANGO_REGISTRY_HOST") {
        registry.host = host;
    }
    if let Ok(ns) = std::env::var("TANGO_REGISTRY_NAMESPACE") {
        registry.namespace = ns;
    }
    if let Ok(user) = std::env::var("TANGO_REGISTRY_USER") {
        registry.username = Some(user);
    }

    // Merge package config if specified
    let (image, package_name, tags) = if let Some(pkg) = package {
        let pkg_cfg = raw.packages.get(pkg)
            .with_context(|| format!("Package '{}' not found in config", pkg))?;
        let pkg_registry = pkg_cfg.registry.as_ref().unwrap_or(&registry);
        registry = pkg_registry.clone();
        let tags = pkg_cfg.defaults.as_ref()
            .map(|d| d.tags.clone())
            .unwrap_or_else(|| raw.defaults.tags.clone());
        (Some(pkg_cfg.image.clone()), Some(pkg.to_string()), tags)
    } else {
        (None, None, raw.defaults.tags.clone())
    };

    // Apply env overrides on host/namespace again after package merge
    if let Ok(host) = std::env::var("TANGO_REGISTRY_HOST") {
        registry.host = host;
    }
    if let Ok(ns) = std::env::var("TANGO_REGISTRY_NAMESPACE") {
        registry.namespace = ns;
    }

    Ok(ResolvedConfig { registry, tags, image, package_name, raw })
}
