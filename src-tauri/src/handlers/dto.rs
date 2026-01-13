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
  pub downloaded_files_cnt: u16,
  #[serde(default)]
  pub total_file_count: u16,
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

pub struct UnzipTask {
  pub file_name: String,
  pub archive_path: PathBuf,
  pub destination_path: PathBuf,
  pub is_latest: bool,
}
