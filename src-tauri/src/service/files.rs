use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::providers::ApiClient::ApiClient::ApiClient;
use crate::{consts::REPO_LAUNCGER_ID, handlers::dto::ReleaseManifest};
use anyhow::{Context, Result};
use futures_util::stream::StreamExt;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

pub type NetSpeedCallback = Box<dyn Fn(&str, u64, f64) + Send + Sync>;

pub struct ServiceFiles {
  callback: Arc<NetSpeedCallback>,
}

impl ServiceFiles {
  pub fn new<F>(callback: F) -> Self
  where
    F: Fn(&str, u64, f64) + Send + Sync + 'static,
  {
    Self {
      callback: Arc::new(Box::new(callback)),
    }
  }

  pub async fn get_launcher_bg(&self, api_client: &ApiClient) -> Result<Vec<u8>> {
    let api = api_client.current_provider()?;

    api.get_launcher_bg().await
  }

  pub async fn download_blob_to_file(
    &self,
    api_client: &ApiClient,
    release_name: &str,
    project_id: &str,
    blob_sha: &str,
    output_path: impl AsRef<Path>,
  ) -> Result<()> {
    let api = api_client.current_provider()?;
    let mut stream = api.get_blob_stream(project_id, blob_sha).await?;

    let mut file = File::create(&output_path).await.context("Failed to create output file")?;

    let start_time = Instant::now();
    let mut downloaded: u64 = 0;
    let mut last_callback = Instant::now();

    while let Some(chunk) = stream.next().await {
      let chunk = chunk.context("Error reading chunk from response stream")?;
      let chunk_len = chunk.len() as u64;

      file.write_all(&chunk).await.context("Failed to write chunk to file")?;
      downloaded += chunk_len;

      let now = Instant::now();
      // Вызываем коллбек не чаще, чем каждые 100 мс (настраиваемо)
      if now.duration_since(last_callback) >= Duration::from_millis(100) {
        let elapsed = now.duration_since(start_time).as_secs_f64();
        let speed = if elapsed > 0.0 { downloaded as f64 / elapsed } else { 0.0 };

        (self.callback)(release_name, downloaded, speed);
        last_callback = now;
      }
    }

    file.flush().await.context("Failed to flush file")?;

    // Финальный вызов коллбека с точными итогами (на случай, если последний чанк был меньше 100 мс назад)
    let elapsed = start_time.elapsed().as_secs_f64();
    let speed = if elapsed > 0.0 { downloaded as f64 / elapsed } else { 0.0 };
    (self.callback)(release_name, downloaded, speed);

    Ok(())
  }
}
