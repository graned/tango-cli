use crate::config::TagDefaults;
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Event {
    /// A commit was pushed (standalone, not a PR)
    Commit,
    /// A pull request was opened/updated
    PullRequest,
    /// A branch was merged to main
    Merge,
}

impl std::str::FromStr for Event {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "commit" => Ok(Event::Commit),
            "pr" | "pull_request" | "pull-request" => Ok(Event::PullRequest),
            "merge" => Ok(Event::Merge),
            other => Err(anyhow::anyhow!("Unknown event type: '{}'", other)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagSet {
    /// Immutable: sha-<commit>
    pub sha_tag: Option<String>,
    /// Mutable: pr-<number>
    pub pr_tag: Option<String>,
    /// Mutable: main
    pub main_tag: Option<String>,
    /// Mutable: latest
    pub latest_tag: Option<String>,
    /// All tags flattened
    pub all: Vec<String>,
}

impl TagSet {
    pub fn generate(event: &Event, sha: &str, pr_number: Option<u64>, cfg: &TagDefaults) -> Self {
        let short_sha = &sha[..sha.len().min(12)];
        let sha_tag = Some(format!("{}{}", cfg.sha_prefix, short_sha));

        let pr_tag = match event {
            Event::PullRequest => pr_number.map(|n| format!("{}{}", cfg.pr_prefix, n)),
            _ => None,
        };

        let main_tag = match event {
            Event::Merge => Some(cfg.main_tag.clone()),
            _ => None,
        };

        let latest_tag = match event {
            Event::Merge if cfg.publish_latest_on_merge => Some(cfg.latest_tag.clone()),
            _ => None,
        };

        let mut all = Vec::new();
        if let Some(t) = &sha_tag {
            all.push(t.clone());
        }
        if let Some(t) = &pr_tag {
            all.push(t.clone());
        }
        if let Some(t) = &main_tag {
            all.push(t.clone());
        }
        if let Some(t) = &latest_tag {
            all.push(t.clone());
        }

        TagSet {
            sha_tag,
            pr_tag,
            main_tag,
            latest_tag,
            all,
        }
    }
}
