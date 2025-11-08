use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
  #[serde(default)]
  pub progress: f32,
  #[serde(default)]
  pub downloaded_files_cnt: u16,
  #[serde(default)]
  pub total_file_count: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifest {
  #[serde(default)]
  pub total_files_count: u32,
  #[serde(default)]
  pub total_size: usize,
  #[serde(default)]
  pub compressed_size: usize,
}
