// Static release index reader.
//
// Per-provider: each provider has its own index with provider-specific URLs.
// The index is fetched with ETag-cached conditional GETs (304 does NOT count
// against the GitHub API rate limit).

use std::{collections::HashMap, time::Duration};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::consts::{
  GITHUB_INDEX_RAW_URL, GITHUB_PID, GITLAB_API_HOST, GITLAB_INDEX_PROJECT_ID, GITLAB_PID, INDEX_CACHE_TTL_SECS, INDEX_SCHEMA_VERSION,
};

// ---------------------------------------------------------------------------
// DTO — kept flat for serde; callers map to provider-specific types.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexAsset {
  pub name: String,
  pub size: u64,
  pub url: String,
  /// Expected SHA-256 copied from the release manifest by the index writer.
  /// None in old indexes → the launcher verifies size only.
  #[serde(default)]
  pub sha256: Option<String>,
  #[serde(default)]
  pub kind: crate::handlers::dto::ManifestFileKind,
  #[serde(default)]
  pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct IndexLauncherAsset {
  pub name: String,
  pub platform: String,
  pub size: u64,
  pub url: String,
  /// Expected SHA-256 of the launcher binary, copied from the release
  /// metadata by the index writer. None in old indexes → the launcher
  /// verifies the downloaded size only.
  #[serde(default)]
  pub sha256: Option<String>,
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
  /// Faction-editor props this patch changes, mirrored from its
  /// `manifest.json`. Only used to flag a patch that is not installed yet;
  /// once installed, the marker and the fragment on disk are the source of
  /// truth. Empty for patches that change no settings.
  #[serde(default)]
  pub updated_fields: Vec<String>,
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
pub struct IndexPreset {
  pub id: String,
  /// `key value` pairs for appdata/user.ltx (written as-is).
  #[serde(default)]
  pub options: HashMap<String, String>,
  /// `key = value` pairs for gamedata/configs/alife.ltx, section [alife] (written as-is).
  #[serde(default)]
  pub alife: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct IndexUserData {
  #[serde(default)]
  pub flags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ReleaseIndex {
  pub schema: u32,
  pub generated_at: String,
  pub launcher: LauncherIndex,
  #[serde(default)]
  pub presets: Vec<IndexPreset>,
  #[serde(default)]
  pub users: HashMap<String, IndexUserData>,
  pub releases: Vec<ReleaseIndexEntry>,
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

/// Resolve the raw URL for the static release index of the given provider.
pub(crate) fn index_raw_url(provider_id: &str) -> Result<String> {
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
  load_index_with_ttl(provider_id, Duration::from_secs(INDEX_CACHE_TTL_SECS)).await
}

/// Like `load_index` but with a caller-supplied TTL.  Pass `Duration::ZERO`
/// to force ETag revalidation (conditional GET) regardless of the normal
/// cache window — cheap for players (GitHub returns 304) and ensures the
/// launcher sees an index published from another machine within seconds.
pub async fn load_index_with_ttl(provider_id: &str, ttl: Duration) -> Result<ReleaseIndex> {
  let url = index_raw_url(provider_id)?;

  let cached = crate::utils::http_cache::fetch(&crate::utils::http_cache::SHARED_CLIENT, &url, ttl).await?;

  let index: ReleaseIndex = serde_json::from_slice(&cached.bytes)?;

  // Only reject indices with a NEWER schema that we cannot parse.
  // Older schemas are read as-is — all new fields carry serde(default).
  if index.schema > INDEX_SCHEMA_VERSION {
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

#[cfg(test)]
mod tests {
  use std::collections::HashMap;

  use super::*;

  #[test]
  fn release_index_users_round_trip() {
    let mut users = HashMap::new();
    users.insert(
      "6e0ead30-48de-4421-99db-cc8b381ad0b3".to_string(),
      IndexUserData {
        flags: vec!["allowPackMod".to_string()],
      },
    );

    let index = ReleaseIndex {
      schema: INDEX_SCHEMA_VERSION,
      generated_at: chrono::Utc::now().to_rfc3339(),
      launcher: LauncherIndex {
        version: "0.0.0".to_string(),
        assets: vec![],
        bg_etag: None,
      },
      presets: vec![],
      users,
      releases: vec![],
    };

    let json = serde_json::to_string(&index).unwrap();
    let parsed: ReleaseIndex = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed.users.len(), 1);
    let user = parsed.users.get("6e0ead30-48de-4421-99db-cc8b381ad0b3").unwrap();
    assert_eq!(user.flags, vec!["allowPackMod".to_string()]);
  }

  #[test]
  fn launcher_asset_sha256_is_optional_and_round_trips() {
    // Old index (published before the hash was added): the asset must still
    // parse, with `sha256: None` so the updater falls back to the size check.
    let old = r#"{"name":"Launcher.exe","platform":"windows","size":123,"url":"https://x/Launcher.exe"}"#;
    let parsed: IndexLauncherAsset = serde_json::from_str(old).unwrap();
    assert_eq!(parsed.sha256, None);

    let new = r#"{"name":"Launcher.exe","platform":"windows","size":123,"url":"https://x/Launcher.exe","sha256":"ABCDEF"}"#;
    let parsed: IndexLauncherAsset = serde_json::from_str(new).unwrap();
    assert_eq!(parsed.sha256.as_deref(), Some("ABCDEF"));

    let json = serde_json::to_string(&parsed).unwrap();
    assert!(json.contains("\"sha256\":\"ABCDEF\""), "sha256 must be published: {}", json);
  }

  #[test]
  fn release_index_users_default_when_missing() {
    let json = r#"{"schema":1,"generated_at":"2026-09-11T00:00:00Z","launcher":{"version":"0.0.0","assets":[]},"releases":[]}"#;
    let parsed: ReleaseIndex = serde_json::from_str(json).unwrap();
    assert!(parsed.users.is_empty());
  }
}
