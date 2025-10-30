use std::path::Path;

use crate::{consts::REPO_LAUNCGER_ID, handlers::dto::ReleaseManifest, service::main::Service};
use anyhow::{Context, Result};
use futures_util::stream::StreamExt;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

pub trait Servicefiles {
  async fn get_launcher_bg(&self) -> Result<Vec<u8>>;
  async fn download_blob_to_file(&self, project_id: &u32, blob_sha: &str, output_path: impl AsRef<Path>) -> Result<()>;
  async fn fetch_manifest_from_blob(&self, project_id: &u32, blob_sha: &str) -> Result<ReleaseManifest>;
}

impl Servicefiles for Service {
  async fn get_launcher_bg(&self) -> Result<Vec<u8>> {
    let api = self.api_client.current_provider()?;

    api
      .get_file_raw(&format!("{}", REPO_LAUNCGER_ID), "data%2Fbg%2Fbg.jpg")
      .await
      .context("Failed to fetch launcher background")
  }

  async fn download_blob_to_file(&self, project_id: &u32, blob_sha: &str, output_path: impl AsRef<Path>) -> Result<()> {
    let api = self.api_client.current_provider()?;
    let mut stream = api.get_blob_stream(project_id, blob_sha).await?;

    let mut file = File::create(&output_path).await.context("Failed to create output file")?;

    while let Some(chunk) = stream.next().await {
      let chunk = chunk.context("Error reading chunk from response stream")?;
      file.write_all(&chunk).await.context("Failed to write chunk to file")?;
    }

    file.flush().await.context("Failed to flush file")?;

    Ok(())
  }

  async fn fetch_manifest_from_blob(&self, project_id: &u32, blob_sha: &str) -> Result<ReleaseManifest> {
    let api = self.api_client.current_provider()?;
    let mut stream = api.get_blob_stream(project_id, blob_sha).await?;

    let mut full_bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
      let bytes = chunk?;
      full_bytes.extend_from_slice(&bytes);
    }

    let json_str = String::from_utf8(full_bytes)?;

    let manifest: ReleaseManifest = serde_json::from_str(&json_str)?;

    Ok(manifest)
  }
}
