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
/// `Completed`   — file fully downloaded, `.part` removed.
/// `Interrupted` — cancelled by the user / shutdown; `.part` saved, must NOT be treated as success.
/// `ShortRead`   — the stream ended early WITHOUT a cancel signal (server closed
///                 the connection, transient network hiccup); `.part` saved.
///                 Callers must treat this as a network error (retry with
///                 backoff), not as a pause — otherwise a flaky connection
///                 masquerades as the user hitting Stop (plan bug fix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadOutcome {
  Completed,
  Interrupted,
  ShortRead,
  /// The stream does not line up with the bytes on disk: a `.part` offset that
  /// outlived its payload, or a server answering 206 from somewhere other than
  /// the requested offset. Seeking anyway would leave a hole of zeros inside a
  /// correctly sized archive — invisible for a release whose manifest carries
  /// no sha256. The partial file and its sidecar are dropped before returning,
  /// so the caller only has to retry.
  RestartRequired,
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
    let (mut stream, stream_start) = api.get_blob_by_url_stream(direct_url, seek).await?;

    let file_name = get_file_name(&output_path).ok_or_else(|| anyhow::anyhow!("download path has no file name"))?;
    let part_file_path = format!(
      "{}.part",
      output_path
        .as_ref()
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("download path is not valid UTF-8"))?
    );

    // Open the target file for writing. When the server ignored the Range
    // header (stream_start == 0 despite a requested resume offset) the body
    // starts from byte 0, so any stale partial bytes must be truncated —
    // appending at the old offset would corrupt the file.
    let requested_offset = seek.unwrap_or(0);

    // The resume offset comes from the `.part` sidecar, which is only a hint:
    // it can outlive the payload (cancelled run, antivirus quarantine, a
    // re-started download that never went through the resume triage). Seeking
    // past the real end of the file makes the OS zero-fill the gap, so the
    // archive ends up the right SIZE but full of holes — and a release whose
    // manifest carries no sha256 has nothing left to catch that: verification
    // passes, unpacking fails, and the file is marked terminally broken.
    //
    // Same for a server that answers 206 from an offset other than the one we
    // asked for. Whenever the stream does not line up with the bytes actually
    // on disk, throw the partial file away and start over.
    let on_disk_len = tokio::fs::metadata(&output_path).await.map(|m| m.len()).unwrap_or(0);
    if stream_start > on_disk_len || (requested_offset > 0 && stream_start != 0 && stream_start != requested_offset) {
      // The bytes between what is on disk and where the stream begins are not
      // coming, so this attempt cannot produce a valid file. Throw the partial
      // payload and the stale offset away and let the caller retry — that
      // attempt will ask for the whole file.
      log::warn!(
        "Restarting {} from scratch: stream starts at {} but the file on disk holds {} bytes (requested offset {})",
        file_name,
        stream_start,
        on_disk_len,
        requested_offset
      );
      let _ = tokio::fs::remove_file(&output_path).await;
      let _ = tokio::fs::remove_file(&part_file_path).await;
      return Ok(DownloadOutcome::RestartRequired);
    }

    let restart_from_zero = requested_offset > 0 && stream_start == 0;
    let mut file = if restart_from_zero || stream_start == 0 {
      // Truncate when starting from the beginning — a stale leftover tail from
      // a previous partial download would cause a redundant full re-download.
      if restart_from_zero {
        log::warn!(
          "Download restart from byte 0 for {}: server ignored the Range request (requested offset {})",
          file_name,
          requested_offset
        );
      }
      OpenOptions::new().write(true).create(true).truncate(true).open(&output_path).await?
    } else {
      OpenOptions::new().write(true).create(true).open(&output_path).await?
    };

    let mut downloaded: u64 = 0;
    if stream_start > 0 {
      file.seek(SeekFrom::Start(stream_start)).await?;
      downloaded = stream_start;
    }
    if restart_from_zero {
      // Drop the stale resume point right away: a crash mid-restart must not
      // resurrect the old (now invalid) offset from the .part sidecar file.
      Self::save_part_file(&part_file_path, 0).await?;
    }

    let start_time = Instant::now();
    // Persist the .part resume point often enough to survive abrupt kills…
    const PART_SAVE_INTERVAL: Duration = Duration::from_millis(250);
    // …but emit progress at ~2 Hz: the frontend rebuilds per-file progress
    // state on every event, so 10 events/s per active file froze the UI on
    // manifests with thousands of files.
    const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(500);
    let mut last_part_save = Instant::now();
    let mut last_emit = Instant::now();
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
        Self::persist_resume_point(&mut file, &part_file_path, downloaded).await?;
        break;
      }

      let chunk = chunk.context("Error reading chunk from response stream")?;
      let chunk_len = chunk.len() as u64;

      file.write_all(&chunk).await.context("Failed to write chunk to file")?;
      downloaded += chunk_len;

      let now = Instant::now();
      if now.duration_since(last_part_save) >= PART_SAVE_INTERVAL {
        // Persist progress periodically so an abrupt process kill keeps the resume point.
        Self::persist_resume_point(&mut file, &part_file_path, downloaded).await?;
        last_part_save = now;
      }

      if now.duration_since(last_emit) >= PROGRESS_EMIT_INTERVAL {
        let elapsed = now.duration_since(start_time).as_secs_f64();
        let speed = if elapsed > 0.0 {
          (downloaded - stream_start) as f64 / elapsed
        } else {
          0.0
        };

        (self.callback)(release_name, &file_name, downloaded, total_bytes.clone(), speed);
        last_emit = now;
      }
    }

    if was_interrupted {
      // Final progress callback so the UI reflects the persisted partial size.
      (self.callback)(release_name, &file_name, downloaded, total_bytes.clone(), 0.0);
      return Ok(DownloadOutcome::Interrupted);
    }

    // Stream ended without cancel — must have the full payload, otherwise this
    // is a short read (server closed the connection early) and must be
    // retried as a network error, not treated as a pause.
    if downloaded < *total_bytes {
      log::warn!(
        "Download short-read for {}: got {} of {} bytes; keeping .part for resume",
        file_name,
        downloaded,
        total_bytes
      );
      Self::persist_resume_point(&mut file, &part_file_path, downloaded).await?;
      (self.callback)(release_name, &file_name, downloaded, total_bytes.clone(), 0.0);
      return Ok(DownloadOutcome::ShortRead);
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
    let mut file = OpenOptions::new().write(true).create(true).truncate(true).open(path).await?;
    file.write_all(downloaded.to_string().as_bytes()).await?;
    // The sidecar is the resume point: if it survives a power loss while its
    // own content is still in the page cache, the next attempt resumes from a
    // byte count that was never recorded. `sync_all` on a ~10-byte file is
    // cheap and runs at most once per PART_SAVE_INTERVAL.
    file.sync_all().await?;
    Ok(())
  }

  /// Flush the payload to the platter and only then record the resume point.
  /// The sidecar must never claim bytes the file does not hold: a `.part` ahead
  /// of its payload resumes into a hole of zeros, which a release without
  /// sha256 has nothing left to catch.
  ///
  /// `sync_data` (not `sync_all`, and NOT per chunk — only when the sidecar is
  /// rewritten) keeps the cost off the hot write path.
  async fn persist_resume_point(file: &mut tokio::fs::File, part_file_path: &str, downloaded: u64) -> Result<()> {
    file.flush().await?;
    file.sync_data().await?;
    Self::save_part_file(part_file_path, downloaded).await
  }
}
