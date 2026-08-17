use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum DownloadStatus {
  Init = 0,
  Pause,
  DownloadFiles,
  Unpacking,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifestFile {
  #[serde(default)]
  pub name: String,
  #[serde(default)]
  pub size: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifest {
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
