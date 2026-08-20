// Static release index reader.
//
// Per-provider: each provider has its own index with provider-specific URLs.
// The index is fetched with ETag-cached conditional GETs (304 does NOT count
// against the GitHub API rate limit).

use std::time::Duration;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::consts::{
  GITHUB_INDEX_RAW_URL, GITHUB_PID, GITLAB_API_HOST, GITLAB_INDEX_PROJECT_ID,
  GITLAB_PID, INDEX_CACHE_TTL_SECS, INDEX_SCHEMA_VERSION,
};

// ---------------------------------------------------------------------------
// DTO — kept flat for serde; callers map to provider-specific types.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexAsset {
    pub name: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct IndexLauncherAsset {
    pub name: String,
    pub platform: String,
    pub size: u64,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LauncherIndex {
    pub version: String,
    pub assets: Vec<IndexLauncherAsset>,
    /// ETag of the launcher background image (bg.jpg).  The player compares
    /// this with the saved value — if they match, the bg is served from disk
    /// with zero network requests.
    #[serde(default)]
    pub bg_etag: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct IndexPatch {
    pub tag: String,
    #[serde(default)]
    pub base_patch: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    #[serde(default)]
    pub manifest: Option<String>,
    #[serde(default)]
    pub assets: Vec<IndexAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReleaseIndexEntry {
    pub name: String,
    pub path: String,
    pub tag: String,
    #[serde(default)]
    pub exe_path: Option<String>,
    pub manifest: String,
    #[serde(default)]
    pub assets: Vec<IndexAsset>,
    #[serde(default)]
    pub patches: Vec<IndexPatch>,
    // Size fields from the release manifest (populated by the writer from CDN).
    // 0 = unknown (e.g. manifest fetch failed during index publish).
    #[serde(default)]
    pub total_files_count: u32,
    #[serde(default)]
    pub total_size: u64,
    #[serde(default)]
    pub compressed_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReleaseIndex {
    pub schema: u32,
    pub generated_at: String,
    pub launcher: LauncherIndex,
    pub releases: Vec<ReleaseIndexEntry>,
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// Resolve the raw URL for the static release index of the given provider.
fn index_raw_url(provider_id: &str) -> Result<String> {
    match provider_id {
        GITHUB_PID => Ok(GITHUB_INDEX_RAW_URL.to_string()),
        GITLAB_PID => {
            if GITLAB_INDEX_PROJECT_ID == 0 {
                bail!("GitLab release index is not configured (GITLAB_INDEX_PROJECT_ID = 0)");
            }
            Ok(format!(
                "{}/projects/{}/repository/files/index.json/raw?ref=master",
                GITLAB_API_HOST, GITLAB_INDEX_PROJECT_ID,
            ))
        }
        _ => bail!("Unknown provider '{}': no release index", provider_id),
    }
}

/// Fetch and parse the static release index for the given provider.
///
/// Uses ETag disk cache (`http_cache`) — a 304 does NOT count against the
/// API rate limit.  Returns `Err` if the index is not configured for this
/// provider, the network is down (and no stale cache exists), or the schema
/// version is incompatible (forces launcher self-update).
pub async fn load_index(provider_id: &str) -> Result<ReleaseIndex> {
    let url = index_raw_url(provider_id)?;

    let cached = crate::utils::http_cache::fetch(
        &crate::utils::http_cache::SHARED_CLIENT,
        &url,
        Duration::from_secs(INDEX_CACHE_TTL_SECS),
    )
    .await?;

    let index: ReleaseIndex = serde_json::from_slice(&cached.bytes)?;

    if index.schema != INDEX_SCHEMA_VERSION {
        bail!(
            "Release index schema {} is not supported (expected {}). \
             Please update the launcher.",
            index.schema,
            INDEX_SCHEMA_VERSION,
        );
    }

    log::info!(
        "Release index loaded for provider '{}' (schema={}, {} releases, cache={:?})",
        provider_id,
        index.schema,
        index.releases.len(),
        cached.source,
    );

    Ok(index)
}
