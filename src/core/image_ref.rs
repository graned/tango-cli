use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A fully-qualified OCI image reference
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageRef {
    pub registry: String,
    pub namespace: String,
    pub name: String,
    pub tag: Option<String>,
    pub digest: Option<String>,
}

impl ImageRef {
    pub fn new(registry: &str, namespace: &str, name: &str) -> Self {
        Self {
            registry: registry.trim_end_matches('/').to_string(),
            namespace: namespace.trim_matches('/').to_string(),
            name: name.to_string(),
            tag: None,
            digest: None,
        }
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self.digest = None;
        self
    }

    pub fn with_digest(mut self, digest: impl Into<String>) -> Self {
        self.digest = Some(digest.into());
        self.tag = None;
        self
    }

    pub fn repository(&self) -> String {
        format!("{}/{}/{}", self.registry, self.namespace, self.name)
    }

    #[allow(dead_code)]
    pub fn validate_digest(digest: &str) -> Result<()> {
        if !digest.starts_with("sha256:") || digest.len() != 71 {
            bail!("Invalid digest '{}': must be sha256:<64-hex-chars>", digest);
        }
        Ok(())
    }
}

impl fmt::Display for ImageRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}/{}", self.registry, self.namespace, self.name)?;
        if let Some(digest) = &self.digest {
            write!(f, "@{}", digest)?;
        } else if let Some(tag) = &self.tag {
            write!(f, ":{}", tag)?;
        }
        Ok(())
    }
}

impl std::str::FromStr for ImageRef {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        // Handle digest reference: repo@sha256:...
        if let Some((repo, digest)) = s.split_once('@') {
            let parts: Vec<&str> = repo.splitn(3, '/').collect();
            if parts.len() != 3 {
                bail!("Invalid image ref '{}': expected registry/namespace/name", s);
            }
            return Ok(ImageRef {
                registry: parts[0].to_string(),
                namespace: parts[1].to_string(),
                name: parts[2].to_string(),
                tag: None,
                digest: Some(digest.to_string()),
            });
        }
        // Handle tag reference: repo:tag
        // Be careful: registry host may contain a port (host:port/ns/name:tag)
        // We need to find the last colon that is after all slashes
        let (repo, tag) = if let Some(last_slash) = s.rfind('/') {
            let after_last_slash = &s[last_slash + 1..];
            if let Some(colon_pos) = after_last_slash.rfind(':') {
                let split_pos = last_slash + 1 + colon_pos;
                (&s[..split_pos], Some(s[split_pos + 1..].to_string()))
            } else {
                (s, None)
            }
        } else if let Some((r, t)) = s.rsplit_once(':') {
            (r, Some(t.to_string()))
        } else {
            (s, None)
        };

        let parts: Vec<&str> = repo.splitn(3, '/').collect();
        if parts.len() != 3 {
            bail!("Invalid image ref '{}': expected registry/namespace/name[:tag]", s);
        }
        Ok(ImageRef {
            registry: parts[0].to_string(),
            namespace: parts[1].to_string(),
            name: parts[2].to_string(),
            tag,
            digest: None,
        })
    }
}
