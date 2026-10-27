use serde::{Deserialize, Serialize};

/// Blob download stream plus the byte offset the stream actually starts from.
/// Offset 0 means the server sent the full body. A server that ignores the
/// Range header answers 200 (not 206) — the caller must restart the file from
/// scratch instead of appending at the requested resume offset.
pub type BlobStreamWithOffset = (
  Box<dyn futures_util::Stream<Item = anyhow::Result<bytes::Bytes>> + Unpin + Send>,
  u64,
);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum ReleasePlatform {
  Windows,
  Linux,
  MacOS,
}

impl ReleasePlatform {
  /// Classify a launcher asset by the name it is published under.
  ///
  /// Accepts the binary file names (`Launcher.exe`, `Launcher`,
  /// `Launcher-macos`), the GitLab release-link labels (`Windows`, `Linux`,
  /// `MacOS`) and the lowercase index `platform` strings. Anything else
  /// returns None: the self-update replaces the RUNNING executable, so a
  /// stray release asset (checksum file, bundle, source archive) must never
  /// be mistaken for a platform binary.
  pub fn from_launcher_name(name: &str) -> Option<ReleasePlatform> {
    match name.to_ascii_lowercase().as_str() {
      "launcher.exe" | "windows" => Some(ReleasePlatform::Windows),
      "launcher" | "linux" => Some(ReleasePlatform::Linux),
      "launcher-macos" | "macos" | "darwin" => Some(ReleasePlatform::MacOS),
      _ => None,
    }
  }
}

#[derive(Serialize, Debug, Clone)]
pub struct ProviderStatus {
  pub available: bool,
  pub latency_ms: Option<u64>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Manifest {
  pub root_id: Option<u32>,
  pub max_size: u64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Release {
  pub id: u32,
  pub name: String,
  pub path: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct ReleaseAssetGit {
  pub name: String,
  pub platform: ReleasePlatform,
  pub size: u64,
  pub download_link: String,
}
#[derive(Deserialize, Debug, Clone)]
pub struct ReleaseGit {
  pub name: String,
  pub version: String,
  pub assets: Vec<ReleaseAssetGit>,
}

/// Asset attached to a release in a concrete repo (used for downloading
/// patch archives and manifest from updates repos).
#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct RepoReleaseAsset {
  pub name: String,
  /// GitHub provides size; GitLab does not (None — must HEAD for size).
  #[serde(default)]
  pub size: Option<u64>,
  pub download_link: String,
}

/// Release of a concrete repo (used for patch chains in updates repos:
/// base_patch detection and release notes).
#[derive(Deserialize, Debug, Clone, Serialize)]
pub struct RepoReleaseInfo {
  pub tag_name: String,
  pub name: String,
  #[serde(default)]
  pub body: Option<String>,
  /// GitHub: date of the tagged commit (the same for all patches of an
  /// updates repo); GitLab: release creation date. Order by `release_date`.
  #[serde(default)]
  pub created_at: Option<String>,
  /// GitHub publication date; `None` on GitLab.
  #[serde(default)]
  pub published_at: Option<String>,
  #[serde(default)]
  pub assets: Vec<RepoReleaseAsset>,
}

impl RepoReleaseInfo {
  /// Date to order releases by (RFC 3339, compares as a string): the
  /// publication date when the provider has one, else `created_at`.
  pub fn release_date(&self) -> &str {
    self.published_at.as_deref().or(self.created_at.as_deref()).unwrap_or("")
  }
}

#[derive(Deserialize, Debug, Clone)]
pub struct Project {
  pub id: u32,
  pub name: String,
  pub path: String,
  pub ssh_remote_url: String,
  #[serde(default)]
  pub marked_for_deletion_on: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TreeItem {
  pub id: String,
  #[serde(skip)]
  pub project_id: String,
  pub name: String,
  pub path: String,
  #[serde(rename = "type")]
  pub item_type: String, // "blob" или "tree"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepoResponse {
  pub id: u32,
  pub name: String,
  pub path: String,
  pub ssh_url_to_repo: String,
  pub visibility: String,
  pub lfs_enabled: bool,
  pub namespace_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreategGroupResponse {
  pub id: u32,
  pub name: String,
  pub path: String,
  pub lfs_enabled: bool,
  pub parent_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
  pub title: String,
  pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReleaseAsset {
  pub file_name: String,
  pub file_download_url: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReleaseResponse {
  pub id: u32,
  pub upload_url: String,
}

/// Server-side hash of a released asset, as reported by the provider
/// (GitHub `digest`, GitLab `file_sha256`). `sha256 = None` when the provider
/// has no hash for the asset (e.g. GitHub assets uploaded before the
/// `digest` field existed).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetSha256 {
  pub name: String,
  #[serde(default)]
  pub size: Option<u64>,
  #[serde(default)]
  pub sha256: Option<String>,
}

#[cfg(test)]
mod tests {
  use super::ReleasePlatform;

  /// The self-update replaces the running executable with the matched asset,
  /// so classification must be exact: known launcher names only, everything
  /// else None. A catch-all bucket would let a checksum or archive file
  /// shadow the real macOS binary in `find()`.
  #[test]
  fn launcher_name_classification_is_exact() {
    // Binary file names (GitHub release assets / index `name`).
    assert_eq!(ReleasePlatform::from_launcher_name("Launcher.exe"), Some(ReleasePlatform::Windows));
    assert_eq!(ReleasePlatform::from_launcher_name("Launcher"), Some(ReleasePlatform::Linux));
    assert_eq!(ReleasePlatform::from_launcher_name("Launcher-macos"), Some(ReleasePlatform::MacOS));
    // GitLab release-link labels (publish-release job).
    assert_eq!(ReleasePlatform::from_launcher_name("Windows"), Some(ReleasePlatform::Windows));
    assert_eq!(ReleasePlatform::from_launcher_name("Linux"), Some(ReleasePlatform::Linux));
    assert_eq!(ReleasePlatform::from_launcher_name("MacOS"), Some(ReleasePlatform::MacOS));
    // Lowercase index `platform` strings (CI jq / index publisher).
    assert_eq!(ReleasePlatform::from_launcher_name("windows"), Some(ReleasePlatform::Windows));
    assert_eq!(ReleasePlatform::from_launcher_name("linux"), Some(ReleasePlatform::Linux));
    assert_eq!(ReleasePlatform::from_launcher_name("macos"), Some(ReleasePlatform::MacOS));
    assert_eq!(ReleasePlatform::from_launcher_name("darwin"), Some(ReleasePlatform::MacOS));
    // Stray release assets must never be treated as a platform binary.
    assert_eq!(ReleasePlatform::from_launcher_name("SHA256SUMS.txt"), None);
    assert_eq!(ReleasePlatform::from_launcher_name("Launcher.dmg"), None);
    assert_eq!(ReleasePlatform::from_launcher_name("Launcher.zip"), None);
    assert_eq!(ReleasePlatform::from_launcher_name(""), None);
  }
}
