use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::providers::ApiClient::ApiClient::ApiClient;
use crate::utils::paths::get_file_name;
use anyhow::{Context, Result};
use futures_util::stream::StreamExt;
use std::io::SeekFrom;
use tokio::fs::OpenOptions;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast::Receiver;

pub type NetSpeedCallback = Box<dyn Fn(&str, &str, u64, u64, f64) + Send + Sync>;

/// Result of a single file download attempt.
/// `Completed`  — file fully downloaded, `.part` removed.
/// `Interrupted` — cancelled by the user / shutdown; `.part` saved, must NOT be treated as success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadOutcome {
  Completed,
  Interrupted,
}

pub struct ServiceFiles {
  callback: Arc<NetSpeedCallback>,
}

impl ServiceFiles {
  pub fn new<F>(callback: F) -> Self
  where
    F: Fn(&str, &str, u64, u64, f64) + Send + Sync + 'static,
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
    direct_url: &str,
    total_bytes: &u64,
    output_path: impl AsRef<Path>,
    seek: &Option<u64>,
    mut rx: Receiver<()>,
  ) -> Result<DownloadOutcome> {
    let api = api_client.current_provider()?;
    let mut stream = api.get_blob_by_url_stream(direct_url, seek).await?;

    let file_name = get_file_name(&output_path).ok_or_else(|| anyhow::anyhow!("download path has no file name"))?;
    let part_file_path = format!(
      "{}.part",
      output_path
        .as_ref()
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("download path is not valid UTF-8"))?
    );

    // Open the target file for writing (append mode for resume)
    let mut file = OpenOptions::new().write(true).create(true).open(&output_path).await?;

    // If resuming, seek to the end of already-downloaded bytes
    let seek_start = seek.unwrap_or(0);
    let mut downloaded: u64 = 0;
    if seek_start > 0 {
      file.seek(SeekFrom::Start(seek_start)).await?;
      downloaded = seek_start;
    }

    let start_time = Instant::now();
    let mut last_callback = Instant::now();
    // Save .part at least every SAVE_INTERVAL to survive abrupt termination.
    const SAVE_INTERVAL: Duration = Duration::from_millis(100);
    let mut was_interrupted = false;

    while let Some(chunk) = stream.next().await {
      // Check for cancellation signal
      let cancelled = match rx.try_recv() {
        Ok(_) => true,
        Err(tokio::sync::broadcast::error::TryRecvError::Empty) => false,
        Err(tokio::sync::broadcast::error::TryRecvError::Closed) => false, // sender gone — keep going, not a cancel
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => true,
      };

      if cancelled {
        log::info!("Download interrupted for file: {}", file_name);
        was_interrupted = true;
        // Flush whatever we have so far, then persist `.part` and the partial file.
        let _ = file.flush().await;
        Self::save_part_file(&part_file_path, downloaded).await?;
        break;
      }

      let chunk = chunk.context("Error reading chunk from response stream")?;
      let chunk_len = chunk.len() as u64;

      file.write_all(&chunk).await.context("Failed to write chunk to file")?;
      downloaded += chunk_len;

      let now = Instant::now();
      if now.duration_since(last_callback) >= SAVE_INTERVAL {
        // Persist progress periodically so an abrupt process kill keeps the resume point.
        Self::save_part_file(&part_file_path, downloaded).await?;

        let elapsed = now.duration_since(start_time).as_secs_f64();
        let speed = if elapsed > 0.0 {
          (downloaded - seek_start) as f64 / elapsed
        } else {
          0.0
        };

        (self.callback)(release_name, &file_name, downloaded, total_bytes.clone(), speed);
        last_callback = now;
      }
    }

    if was_interrupted {
      // Final progress callback so the UI reflects the persisted partial size.
      (self.callback)(release_name, &file_name, downloaded, total_bytes.clone(), 0.0);
      return Ok(DownloadOutcome::Interrupted);
    }

    // Stream ended without cancel — must have the full payload or treat as interrupt.
    if downloaded < *total_bytes {
      log::warn!(
        "Download short-read for {}: got {} of {} bytes; keeping .part for resume",
        file_name,
        downloaded,
        total_bytes
      );
      let _ = file.flush().await;
      Self::save_part_file(&part_file_path, downloaded).await?;
      (self.callback)(release_name, &file_name, downloaded, total_bytes.clone(), 0.0);
      return Ok(DownloadOutcome::Interrupted);
    }

    file.flush().await?;

    // Download finished successfully — remove `.part`.
    let _ = tokio::fs::remove_file(&part_file_path).await;

    (self.callback)(release_name, &file_name, downloaded, total_bytes.clone(), 0.0);
    Ok(DownloadOutcome::Completed)
  }

  // Persist the current downloaded byte count into a `.part` sidecar file.
  async fn save_part_file(path: &str, downloaded: u64) -> Result<()> {
    // Write number as string — robust and easy to debug.
    tokio::fs::write(path, downloaded.to_string().as_bytes()).await?;
    Ok(())
  }
}
