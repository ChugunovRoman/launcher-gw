use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum DownloadStatus {
  Init = 0,
  Pause,
  DownloadFiles,
  Unpacking,
  Verifying,
  Error,
}

#[derive(Clone, Serialize)]
pub struct ProgressPayload {
  pub version_name: String,
  pub file_name: String,
  pub bytes_moved: u64,
  pub total_bytes: u64,
  pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
  #[serde(default)]
  pub version_name: String,
  pub status: DownloadStatus,
  #[serde(default)]
  pub progress: f32,
  #[serde(default)]
  pub file: String,
  #[serde(default)]
  pub downloaded_files_cnt: u32,
  #[serde(default)]
  pub total_file_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownlaodFileStat {
  #[serde(default)]
  pub name: String,
  #[serde(default)]
  pub unpacked: bool,
  #[serde(default)]
  pub size: Option<u64>,
}

/// How a manifest entry is processed after downloading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestFileKind {
  /// Unpack the zip into the install dir, then delete the archive.
  #[default]
  Zip,
  /// Copy the file as-is into `<install>/<target>` (or `<name>` when target is
  /// empty) — for future engine `.db*`/`.xdb` archives that need no unpacking.
  Raw,
  /// The patch `manifest.json` itself, stored in `files` as an asset; it is
  /// metadata, not download data.
  Manifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifestFile {
  #[serde(default)]
  pub name: String,
  #[serde(default)]
  pub size: u64,
  /// SHA-256 of the finished output file (the `dataN.zip` / `.db` itself, not
  /// the archive contents), lowercase hex. None = old manifest without hashes.
  #[serde(default)]
  pub sha256: Option<String>,
  #[serde(default)]
  pub kind: ManifestFileKind,
  /// Raw files only: path inside the install dir, '/' separators.
  #[serde(default)]
  pub target: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReleaseManifest {
  /// 0 = legacy (no hashes), 2 = sha256 + kind/target manifest (this schema).
  #[serde(default)]
  pub schema: u32,
  #[serde(default)]
  pub total_files_count: u32,
  #[serde(default)]
  pub total_size: u64,
  #[serde(default)]
  pub compressed_size: u64,
  #[serde(default)]
  pub files: Vec<ReleaseManifestFile>,
  #[serde(default)]
  pub exe_path: Option<String>,
  // Partial-update patch metadata. Absent (None/empty) for full release
  // manifests, so old manifests keep deserializing unchanged.
  /// Patch name = release tag in the updates repo.
  #[serde(default)]
  pub patch_name: Option<String>,
  /// Previous patch in the chain (None = first patch after a full release).
  #[serde(default)]
  pub base_patch: Option<String>,
  /// Tag of the full release the patch chain is based on.
  #[serde(default)]
  pub base_release_tag: Option<String>,
  /// Files to delete when applying the patch, relative to the game root.
  #[serde(default)]
  pub deleted_files: Vec<String>,
  /// Faction-editor props this patch changes. Carried here so that
  /// re-publishing the index (which rebuilds every entry from the patch
  /// manifests) cannot lose it — see the plan §2.4.
  #[serde(default)]
  pub updated_fields: Vec<String>,
}

/// Patch metadata passed to the packer when building a patch upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchMeta {
  pub patch_name: String,
  pub base_patch: Option<String>,
  pub base_release_tag: Option<String>,
  #[serde(default)]
  pub deleted_files: Vec<String>,
  #[serde(default)]
  pub updated_fields: Vec<String>,
}

#[derive(Clone, Serialize)]
pub struct CompressProgressPayload {
  pub status: u8,
  pub current_file: String,
  pub total_size: u64,
  pub processed_size: u64,
  pub percentage: f64,
}

#[derive(Clone, Serialize)]
pub struct UploadProgressPayload {
  pub file_name: String,
  pub file_uploaded_size: u64,
  pub file_total_size: u64,
  pub total_uploaded_size: u64,
  pub total_size: u64,
  pub speed: f64,
}

#[derive(Debug)]
pub struct UnzipTask {
  pub file_name: String,
  pub archive_path: PathBuf,
  pub destination_path: PathBuf,
}

/// Post-processing dispatched after a downloaded file passes verification.
/// `Unzip` — extract the zip into `destination_path` (a directory), then the
/// archive is deleted. `Copy` — move a raw file to `destination_path` (a full
/// file path inside the install dir), no unpacking (engine `.db*` archives).
#[derive(Debug)]
pub enum PostProcessTask {
  Unzip(UnzipTask),
  Copy(UnzipTask),
}

/// Error payload of the `download-version-file-error` event.
#[derive(Debug, Clone, Serialize)]
pub struct FileErrorPayload {
  pub version_name: String,
  pub file: String,
  /// One of the FILE_ERR_* constants from consts.rs.
  pub code: String,
  pub message: String,
}
