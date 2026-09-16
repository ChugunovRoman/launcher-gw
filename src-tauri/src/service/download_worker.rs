// Shared download worker: one implementation of the download → verify →
// post-process pipeline used by start_download_version,
// continue_download_version and the installed-version repair.
//
// Extracted from the two hand-copied worker loops that used to live in the
// start/continue commands (plan P6). Retry policy (plan P2/P3):
// - per-file `net_retries` (MAX_DOWNLOAD_RETRIES) for network errors;
// - per-file `verify_retries` (MAX_VERIFY_RETRIES) for size/hash mismatches;
// - exponential backoff 2s → 4s → 8s (capped at 15s);
// - an exhausted file is marked `last_error` and the queue CONTINUES with the
//   remaining files; the version ends in DownloadStatus::Error and the command
//   returns Err(DOWNLOAD_FAILED) instead of masquerading as a pause.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tauri::Emitter;
use tokio::sync::{Mutex, mpsc};

use crate::configs::AppConfig::{AppConfig, FileProgress, VersionProgress};
use crate::consts::{
  FILE_ERR_BAD_MANIFEST, FILE_ERR_COPY_FAILED, FILE_ERR_HASH_MISMATCH, FILE_ERR_NETWORK, FILE_ERR_SIZE_MISMATCH, FILE_ERR_UNPACK_FAILED, FILE_ERR_VERIFY_FAILED, MAX_DOWNLOAD_RETRIES,
  MAX_VERIFY_RETRIES, MAX_DOWNLOAD_ATTEMPTS_PER_FILE, PULL_FILES_SIZE, ERR_DOWNLOAD_FAILED, ERR_USER_CANCELLED,
};
use crate::handlers::dto::{DownloadProgress, DownloadStatus, FileErrorPayload, PostProcessTask};
use crate::providers::ApiClient::ApiClient::ApiClient;
use crate::service::files::{DownloadOutcome, ServiceFiles};
use crate::service::unpack::ServiceUnpacker;

/// Result of verifying a downloaded file against the manifest.
#[derive(Debug, Clone)]
pub enum VerifyResult {
  Ok,
  SizeMismatch { expected: u64, actual: u64 },
  HashMismatch { expected: String, actual: String },
  /// No sha256 in the manifest (legacy release): size was still checked.
  Skipped,
}

/// Verify a finished download: cheap size check first, then SHA-256 by
/// re-reading the file (see utils::hash for why re-reading, not streaming).
pub async fn verify_downloaded_file(path: &Path, expected_size: u64, expected_sha256: Option<&str>) -> anyhow::Result<VerifyResult> {
  // A zero-length entry is a manifest defect, not a file: without this guard an
  // empty (or never written) file matches `expected_size` and is accepted as
  // downloaded, and the install silently ends up missing that payload.
  if expected_size == 0 {
    anyhow::bail!("manifest declares zero size for {:?}", path);
  }
  let meta = tokio::fs::metadata(path).await?;
  if meta.len() != expected_size {
    return Ok(VerifyResult::SizeMismatch { expected: expected_size, actual: meta.len() });
  }
  let Some(expected) = expected_sha256 else {
    log::warn!("verify: no sha256 in manifest for {:?}, checking size only", path);
    return Ok(VerifyResult::Skipped);
  };
  let p = path.to_path_buf();
  let actual = tokio::task::spawn_blocking(move || crate::utils::hash::sha256_file(&p, None, None)).await??;
  if actual.eq_ignore_ascii_case(expected) {
    Ok(VerifyResult::Ok)
  } else {
    Ok(VerifyResult::HashMismatch { expected: expected.to_string(), actual })
  }
}

/// Same as `verify_downloaded_file`, but emits `download-version-file-verify`
/// progress events (~2 Hz) and flips the version status to Verifying.
/// `downloaded_cnt`/`total_cnt` keep the version-level counters intact in
/// the status event (they are read before the file is counted as done).
pub async fn verify_downloaded_file_emit(
  app: &tauri::AppHandle,
  version_name: &str,
  file_name: &str,
  path: &Path,
  expected_size: u64,
  expected_sha256: Option<&str>,
  downloaded_cnt: u32,
  total_cnt: u32,
) -> anyhow::Result<VerifyResult> {
  // See `verify_downloaded_file`: size 0 is a broken manifest entry.
  if expected_size == 0 {
    anyhow::bail!("manifest declares zero size for '{}'", file_name);
  }

  if expected_sha256.is_some() {
    let _ = app.emit(
      "download-version",
      DownloadProgress {
        version_name: version_name.to_string(),
        status: DownloadStatus::Verifying,
        file: file_name.to_string(),
        progress: if total_cnt > 0 { (downloaded_cnt as f32 / total_cnt as f32) * 100.0 } else { 0.0 },
        downloaded_files_cnt: downloaded_cnt,
        total_file_count: total_cnt,
      },
    );
  }

  let meta = tokio::fs::metadata(path).await?;
  if meta.len() != expected_size {
    return Ok(VerifyResult::SizeMismatch { expected: expected_size, actual: meta.len() });
  }
  let Some(expected) = expected_sha256 else {
    log::warn!("verify: no sha256 in manifest for '{}', checking size only", file_name);
    return Ok(VerifyResult::Skipped);
  };

  let path = path.to_path_buf();
  let app_c = app.clone();
  let version_c = version_name.to_string();
  let file_c = file_name.to_string();
  let actual = tokio::task::spawn_blocking(move || {
    // ~2 Hz throttle: hashing a 2 GiB part fires hundreds of callbacks.
    let last_emit = std::cell::Cell::new(std::time::Instant::now() - Duration::from_secs(1));
    let on_progress = move |done: u64, total: u64| {
      let now = std::time::Instant::now();
      if now.duration_since(last_emit.get()) >= Duration::from_millis(500) {
        last_emit.set(now);
        let _ = app_c.emit("download-version-file-verify", (&version_c, &file_c, done, total));
      }
    };
    crate::utils::hash::sha256_file(&path, Some(&on_progress), None)
  })
  .await??;

  if actual.eq_ignore_ascii_case(expected) {
    Ok(VerifyResult::Ok)
  } else {
    Ok(VerifyResult::HashMismatch { expected: expected.to_string(), actual })
  }
}

/// State shared by the download workers of one version.
pub struct DownloadWorkerShared {
  pub app: tauri::AppHandle,
  pub app_config: Arc<Mutex<AppConfig>>,
  pub service_files: Arc<ServiceFiles>,
  pub api_client: ApiClient,
  pub version_name: String,
  pub download_dir: PathBuf,
  pub install_path: PathBuf,
  pub total_file_count: u32,
  pub downloaded_cnt: Arc<std::sync::atomic::AtomicU32>,
  pub cancel: crate::handlers::start_download_version::CancelHandle,
  /// Timestamp of the last config save; used to debounce writes.
  pub last_save: std::sync::Mutex<std::time::Instant>,
}

/// Minimum interval between config saves.
const SAVE_DEBOUNCE: Duration = Duration::from_secs(1);

/// Save config with debouncing: skip if less than SAVE_DEBOUNCE has elapsed
/// since the last save. Only counter bookkeeping goes through here — the saves
/// that MUST reach the disk (terminal file error, file marked downloaded,
/// resume point, end of pipeline) call `AppConfig::save()` directly through
/// `mark_file_error` / `update_config_and_emit` / `persist_file_size` /
/// `finalize_download`, so no bypass flag is needed.
async fn debounced_save(shared: &Arc<DownloadWorkerShared>) {
  let elapsed = shared.last_save.lock().unwrap().elapsed();
  if elapsed < SAVE_DEBOUNCE {
    return;
  }
  let cfg = shared.app_config.lock().await;
  let _ = cfg.save();
  *shared.last_save.lock().unwrap() = std::time::Instant::now();
}

/// Exponential backoff between retries: 2s, 4s, 8s, … capped at 15s.
fn backoff_delay(attempt: u32) -> Duration {
  Duration::from_secs((2u64).saturating_pow(attempt.min(4)).min(15))
}

/// Record a terminal per-file error: persists `last_error`, emits
/// `download-version-file-error`. The queue keeps going with other files.
/// Standalone variant usable before a `DownloadWorkerShared` exists (the
/// files_to_postprocess pass of `run_version_pipeline`).
async fn mark_file_error_direct(app: &tauri::AppHandle, app_config: &Arc<Mutex<AppConfig>>, version_name: &str, file_name: &str, code: &str, message: String) {
  {
    let mut cfg = app_config.lock().await;
    if let Some(ver) = cfg.progress_download.get_mut(version_name) {
      if let Some(fp) = ver.files.get_mut(file_name) {
        fp.last_error = Some(code.to_string());
        fp.is_downloaded = false;
      }
    }
    let _ = cfg.save();
  }
  let _ = app.emit(
    "download-version-file-error",
    FileErrorPayload {
      version_name: version_name.to_string(),
      file: file_name.to_string(),
      code: code.to_string(),
      message,
    },
  );
}

async fn mark_file_error(shared: &DownloadWorkerShared, file_name: &str, code: &str, message: String) {
  mark_file_error_direct(&shared.app, &shared.app_config, &shared.version_name, file_name, code, message).await;
}

/// Read the resume offset recorded in a `.part` sidecar (0 when it is missing
/// or unreadable). Sync I/O, hence `spawn_blocking`.
async fn read_part_offset(part_path: &str) -> u64 {
  let part_path_owned = part_path.to_owned();
  tokio::task::spawn_blocking(move || {
    std::fs::read_to_string(&part_path_owned)
      .ok()
      .and_then(|s| s.trim().parse::<u64>().ok())
      .unwrap_or(0)
  })
  .await
  .unwrap_or(0)
}

/// `net_retries` after a failed network attempt. The limit counts CONSECUTIVE
/// fruitless attempts: an attempt that appended at least one byte resets the
/// counter, otherwise five hiccups spread over a multi-gigabyte file (each one
/// having moved the download hundreds of megabytes forward) would kill it.
fn next_net_retries(current: u32, offset_before: u64, offset_after: u64) -> u32 {
  if offset_after > offset_before { 0 } else { current + 1 }
}

/// Persist the `.part` byte count into `FileProgress.size` so the resume point
/// survives an abrupt kill. Called after interruptions and failed attempts.
/// `part_path` must be a real path: callers that could not build one must NOT
/// call this, or the unreadable path would overwrite `size` with 0 and throw
/// the resume point away.
pub async fn persist_file_size(config: &Arc<Mutex<AppConfig>>, version_name: &str, file_name: &str, part_path: &str) {
  if part_path.is_empty() || part_path == ".part" {
    log::warn!("persist_file_size: refusing to persist size for '{}' — no valid .part path", file_name);
    return;
  }
  let size = read_part_offset(part_path).await;

  let mut config_guard = config.lock().await;
  if let Some(ver) = config_guard.progress_download.get_mut(version_name) {
    if let Some(fp) = ver.files.get_mut(file_name) {
      fp.size = size;
    }
  }
  let _ = config_guard.save();
}

/// Mark a file downloaded in the config and emit the version progress event.
async fn update_config_and_emit(app: &tauri::AppHandle, config: &Arc<Mutex<AppConfig>>, version_name: &str, file_name: &str, current: u32, total: u32) {
  let mut config_guard = config.lock().await;
  if let Some(ver) = config_guard.progress_download.get_mut(version_name) {
    if let Some(file_progress) = ver.files.get_mut(file_name) {
      file_progress.is_downloaded = true;
    }
    ver.downloaded_files_cnt = current;
  }
  let _ = config_guard.save();

  // Guard against division by zero when total is 0 (e.g. empty manifest).
  let progress = if total > 0 { (current as f32 / total as f32) * 100.0 } else { 0.0 };
  let _ = app.emit(
    "download-version",
    DownloadProgress {
      version_name: version_name.to_string(),
      status: DownloadStatus::DownloadFiles,
      file: file_name.to_string(),
      progress,
      downloaded_files_cnt: current,
      total_file_count: total,
    },
  );
}

/// Undo the "file done" bookkeeping after post-processing failed terminally.
/// The counter is bumped as soon as the payload is on disk, so without this the
/// UI reports "40 / 40 files" right next to a download error.
async fn rollback_downloaded_count(
  app: &tauri::AppHandle,
  config: &Arc<Mutex<AppConfig>>,
  downloaded_cnt: &Arc<std::sync::atomic::AtomicU32>,
  version_name: &str,
  file_name: &str,
  total: u32,
) {
  let current = downloaded_cnt
    .fetch_update(std::sync::atomic::Ordering::SeqCst, std::sync::atomic::Ordering::SeqCst, |v| v.checked_sub(1))
    .map(|prev| prev.saturating_sub(1))
    .unwrap_or(0);

  {
    let mut config_guard = config.lock().await;
    if let Some(ver) = config_guard.progress_download.get_mut(version_name) {
      ver.downloaded_files_cnt = current;
    }
    let _ = config_guard.save();
  }

  let progress = if total > 0 { (current as f32 / total as f32) * 100.0 } else { 0.0 };
  let _ = app.emit(
    "download-version",
    DownloadProgress {
      version_name: version_name.to_string(),
      status: DownloadStatus::DownloadFiles,
      file: file_name.to_string(),
      progress,
      downloaded_files_cnt: current,
      total_file_count: total,
    },
  );
}

/// Spawn PULL_FILES_SIZE download workers over the shared queue. Returns when
/// every worker exits (queue drained, cancelled, or all files done). Retries
/// are held locally by the worker (single worker today), so the queue channel
/// is not needed for re-queuing.
pub async fn run_download_workers(
  shared: Arc<DownloadWorkerShared>,
  rx_queue: mpsc::Receiver<FileProgress>,
  tx_unzip: mpsc::Sender<PostProcessTask>,
) {
  let rx_queue = Arc::new(Mutex::new(rx_queue));
  let mut join_handles = Vec::new();

  for _ in 0..PULL_FILES_SIZE {
    let shared = shared.clone();
    let tx_unzip_c = tx_unzip.clone();
    let rx_queue_c = rx_queue.clone();
    let mut stop_rx = shared.cancel.subscribe();
    let handle = tokio::spawn(async move {
      // Per-file retry counters travel INSIDE the FileProgress task struct, so
      // they survive re-queuing and are persisted to the config.
      let mut current_task: Option<FileProgress> = None;
      // Hard ceiling on attempts for ONE file within this session. `net_retries`
      // counts consecutive fruitless attempts and resets on any progress, which
      // is what a flaky connection needs — but a server trickling a byte and
      // dropping would then reset the budget forever. This counter never
      // resets while the same file is being retried.
      let mut attempts_this_file: u32 = 0;

      loop {
        // The flag, not the channel: a cancel sent before this worker
        // subscribed is invisible to `try_recv` but still set on the flag.
        if shared.cancel.is_cancelled() {
          if let Some(task) = &current_task {
            // Only persist when the `.part` path really resolves: a failed join
            // used to degrade into the relative path ".part", which reads back
            // as 0 bytes and wipes the stored resume point.
            match crate::utils::paths::safe_download_join(&shared.download_dir, &task.name) {
              Ok(p) => {
                let part = format!("{}.part", p.to_string_lossy());
                persist_file_size(&shared.app_config, &shared.version_name, &task.name, &part).await;
              }
              Err(e) => log::warn!("Cancel: cannot build .part path for '{}', size not persisted: {}", &task.name, e),
            }
          }
          break;
        }

        let mut task = match current_task.take() {
          Some(t) => t,
          None => {
            // A brand-new file: reset the per-file attempt ceiling below.
            attempts_this_file = 0;
            let mut rx_lock = rx_queue_c.lock().await;
            tokio::select! {
              _ = stop_rx.recv() => break,
              task = rx_lock.recv() => match task {
                Some(t) => t,
                None => break,
              }
            }
          }
        };

        // A manifest entry without a size can never verify (see
        // `verify_downloaded_file`), so fail it terminally instead of burning
        // the retry budget on it.
        if task.total_size == 0 {
          log::error!("Manifest entry '{}' declares zero size", &task.name);
          mark_file_error(&shared, &task.name, FILE_ERR_BAD_MANIFEST, "manifest declares zero size".to_string()).await;
          continue;
        }

        let file_path = match crate::utils::paths::safe_download_join(&shared.download_dir, &task.name) {
          Ok(p) => p,
          Err(e) => {
            log::error!("safe_download_join failed: {}", e);
            mark_file_error(&shared, &task.name, FILE_ERR_COPY_FAILED, e.to_string()).await;
            continue;
          }
        };
        // A non-UTF-8 path would degrade into the relative ".part" here and
        // later wipe the resume point, so treat it as a bad manifest entry.
        let part_path = match file_path.to_str() {
          Some(p) => format!("{}.part", p),
          None => {
            log::error!("Download path for '{}' is not valid UTF-8", &task.name);
            mark_file_error(&shared, &task.name, FILE_ERR_BAD_MANIFEST, "download path is not valid UTF-8".to_string()).await;
            continue;
          }
        };

        attempts_this_file = attempts_this_file.saturating_add(1);

        // Read existing progress for the Range header.
        let offset_before = read_part_offset(&part_path).await;
        let seek_pos = if offset_before > 0 { Some(offset_before) } else { None };

        let mut local_cancel = shared.cancel.subscribe();
        let res = shared
          .service_files
          .download_blob_to_file(
            &shared.api_client,
            &shared.version_name,
            &task.download_link,
            &task.total_size,
            &file_path,
            &seek_pos,
            local_cancel,
          )
          .await;

        match res {
          Ok(DownloadOutcome::Completed) => {
            let current_before = shared.downloaded_cnt.load(std::sync::atomic::Ordering::SeqCst);
            let verify = verify_downloaded_file_emit(
              &shared.app,
              &shared.version_name,
              &task.name,
              &file_path,
              task.total_size,
              task.sha256.as_deref(),
              current_before,
              shared.total_file_count,
            )
            .await;

            match verify {
              Ok(VerifyResult::Ok) | Ok(VerifyResult::Skipped) => {
                task.net_retries = 0;
                task.verify_retries = 0;

                // The file is complete: drop the resume sidecar right here.
                // It used to survive the whole pipeline, and the archive is
                // deleted after unpacking — so the next resume found a
                // sidecar with no payload next to it, read that as "a partial
                // download whose file vanished" and re-downloaded a version
                // that was already installed, while the UI kept showing it as
                // done.
                let _ = tokio::fs::remove_file(&part_path).await;

                {
                  let mut cfg = shared.app_config.lock().await;
                  if let Some(ver) = cfg.progress_download.get_mut(&shared.version_name) {
                    if let Some(fp) = ver.files.get_mut(&task.name) {
                      fp.net_retries = 0;
                      fp.verify_retries = 0;
                      fp.last_error = None;
                    }
                  }
                }
                debounced_save(&shared).await;

                // Dispatch post-processing by file kind (zip → unpack, raw → copy).
                // Built BEFORE the counters below so a bad `target` is reported
                // as a terminal file error rather than silently escaping the
                // install dir.
                let post_result = post_process_task(&task, &file_path, &shared.install_path);

                let current = shared.downloaded_cnt.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                update_config_and_emit(&shared.app, &shared.app_config, &shared.version_name, &task.name, current, shared.total_file_count).await;

                match post_result {
                  Ok(post_task) => {
                    if tx_unzip_c.send(post_task).await.is_err() {
                      log::error!("Post-process channel closed for '{}'", &task.name);
                    }
                  }
                  Err(e) => {
                    // mark_file_error runs AFTER update_config_and_emit above so
                    // its `is_downloaded = false` / `last_error` win over the
                    // "downloaded" flag update_config_and_emit just wrote.
                    log::error!("Invalid raw target for '{}': {}", &task.name, &e);
                    mark_file_error(&shared, &task.name, FILE_ERR_COPY_FAILED, e).await;
                    // …and the file is not done after all: give the counter its
                    // slot back, otherwise the UI shows "40/40" next to the
                    // download error.
                    rollback_downloaded_count(&shared.app, &shared.app_config, &shared.downloaded_cnt, &shared.version_name, &task.name, shared.total_file_count).await;
                  }
                }

                // Re-read the counter: the post-process dispatch above may have
                // rolled it back, and a stale `current` would end the worker
                // while files are still queued.
                if shared.downloaded_cnt.load(std::sync::atomic::Ordering::SeqCst) >= shared.total_file_count {
                  // Just leave: the queue sender was dropped before the workers
                  // started, so the others end on their own.  Signalling cancel
                  // here would now abort the post-process queue as well.
                  break;
                }
              }
              Ok(mismatch) => {
                let (code, message) = match &mismatch {
                  VerifyResult::SizeMismatch { expected, actual } => (
                    FILE_ERR_SIZE_MISMATCH,
                    format!("size mismatch: expected {} bytes, got {}", expected, actual),
                  ),
                  VerifyResult::HashMismatch { expected, actual } => (
                    FILE_ERR_HASH_MISMATCH,
                    format!("sha256 mismatch: expected {}, got {}", expected, actual),
                  ),
                  _ => unreachable!("Ok/Skipped handled above"),
                };
                log::error!("Verify failed for '{}': {}", &task.name, &message);

                // Delete the corrupt file and its resume point, then either
                // re-queue or record the terminal error.
                let _ = tokio::fs::remove_file(&file_path).await;
                let _ = tokio::fs::remove_file(&part_path).await;

                task.verify_retries += 1;
                {
                  let mut cfg = shared.app_config.lock().await;
                  if let Some(ver) = cfg.progress_download.get_mut(&shared.version_name) {
                    if let Some(fp) = ver.files.get_mut(&task.name) {
                      fp.verify_retries = task.verify_retries;
                      fp.is_downloaded = false;
                    }
                  }
                }
                debounced_save(&shared).await;

                if task.verify_retries <= MAX_VERIFY_RETRIES {
                  log::warn!(
                    "Verify attempt {}/{} failed for '{}', re-downloading from scratch",
                    task.verify_retries, MAX_VERIFY_RETRIES, &task.name
                  );
                  tokio::time::sleep(backoff_delay(task.verify_retries)).await;
                  current_task = Some(task);
                } else {
                  mark_file_error(&shared, &task.name, code, format!("{} (after {} attempts)", message, MAX_VERIFY_RETRIES)).await;
                  // The queue continues with the remaining files (plan Q6).
                }
              }
              Err(e) => {
                // I/O error while hashing (AV lock, disk): retry like a
                // mismatch, but with its own error code.
                log::warn!("Verify of '{}' errored: {}", &task.name, e);
                task.verify_retries += 1;
                {
                  let mut cfg = shared.app_config.lock().await;
                  if let Some(ver) = cfg.progress_download.get_mut(&shared.version_name) {
                    if let Some(fp) = ver.files.get_mut(&task.name) {
                      fp.verify_retries = task.verify_retries;
                      fp.is_downloaded = false;
                    }
                  }
                }
                debounced_save(&shared).await;
                if task.verify_retries <= MAX_VERIFY_RETRIES {
                  tokio::time::sleep(backoff_delay(task.verify_retries)).await;
                  current_task = Some(task);
                } else {
                  mark_file_error(&shared, &task.name, FILE_ERR_VERIFY_FAILED, e.to_string()).await;
                }
              }
            }
          }
          Ok(DownloadOutcome::Interrupted) => {
            log::info!("Download of '{}' interrupted by cancel signal, saving progress", task.name);
            persist_file_size(&shared.app_config, &shared.version_name, &task.name, &part_path).await;
            break;
          }
          Ok(DownloadOutcome::RestartRequired) => {
            // The partial file and its sidecar were inconsistent and have been
            // dropped. Re-queue WITHOUT counting a network failure: nothing
            // went wrong on the wire, and the next attempt asks for the whole
            // file from byte 0.
            log::warn!("Re-queuing '{}' from byte 0: resume state was inconsistent", &task.name);
            current_task = Some(task);
          }
          Ok(DownloadOutcome::ShortRead) => {
            // The stream ended early WITHOUT a cancel signal — a real network
            // error, not a pause. Route it through the same net_retries/backoff
            // path as an Err() below instead of breaking the worker.
            let offset_after = read_part_offset(&part_path).await;
            if offset_after > offset_before {
              log::info!("Short read for '{}' after {} new bytes, retry budget reset", task.name, offset_after - offset_before);
            }
            task.net_retries = next_net_retries(task.net_retries, offset_before, offset_after);
            {
              let mut cfg = shared.app_config.lock().await;
              if let Some(ver) = cfg.progress_download.get_mut(&shared.version_name) {
                if let Some(fp) = ver.files.get_mut(&task.name) {
                  fp.net_retries = task.net_retries;
                }
              }
            }
            debounced_save(&shared).await;
            if task.net_retries > MAX_DOWNLOAD_RETRIES || attempts_this_file > MAX_DOWNLOAD_ATTEMPTS_PER_FILE {
              log::error!("Download of '{}' failed after {} attempts: short read (server closed the connection early)", task.name, MAX_DOWNLOAD_RETRIES);
              mark_file_error(&shared, &task.name, FILE_ERR_NETWORK, "short read: server closed the connection early".to_string()).await;
              // The queue continues with the remaining files.
            } else {
              log::warn!("Short read for '{}' (attempt {}/{}). Retrying...", task.name, task.net_retries, MAX_DOWNLOAD_RETRIES);
              persist_file_size(&shared.app_config, &shared.version_name, &task.name, &part_path).await;
              tokio::time::sleep(backoff_delay(task.net_retries)).await;
              current_task = Some(task);
            }
          }
          Err(e) => {
            // Same consecutive-failure accounting as the ShortRead arm above:
            // an attempt that moved the resume point forward does not count
            // against MAX_DOWNLOAD_RETRIES.
            let offset_after = read_part_offset(&part_path).await;
            if offset_after > offset_before {
              log::info!("Download of '{}' failed after {} new bytes, retry budget reset", task.name, offset_after - offset_before);
            }
            task.net_retries = next_net_retries(task.net_retries, offset_before, offset_after);
            {
              let mut cfg = shared.app_config.lock().await;
              if let Some(ver) = cfg.progress_download.get_mut(&shared.version_name) {
                if let Some(fp) = ver.files.get_mut(&task.name) {
                  fp.net_retries = task.net_retries;
                }
              }
            }
            debounced_save(&shared).await;
            if task.net_retries > MAX_DOWNLOAD_RETRIES || attempts_this_file > MAX_DOWNLOAD_ATTEMPTS_PER_FILE {
              log::error!("Download of '{}' failed after {} attempts: {}", task.name, MAX_DOWNLOAD_RETRIES, e);
              mark_file_error(&shared, &task.name, FILE_ERR_NETWORK, e.to_string()).await;
              // The queue continues with the remaining files.
            } else {
              log::warn!("Error downloading '{}' (attempt {}/{}): {}. Retrying...", task.name, task.net_retries, MAX_DOWNLOAD_RETRIES, e);
              // Persist partial size so a kill mid-retry keeps the resume point.
              persist_file_size(&shared.app_config, &shared.version_name, &task.name, &part_path).await;
              tokio::time::sleep(backoff_delay(task.net_retries)).await;
              current_task = Some(task);
            }
          }
        }
      }
    });
    join_handles.push(handle);
  }

  for h in join_handles {
    let _ = h.await;
  }
}

/// Build the post-process task for a verified file:
/// zip → unpack into the install dir; raw → move to install/<target>.
/// Returns `Err` when a raw file's `target` escapes the install dir (`..`,
/// absolute path, drive letter) — the caller must NOT dispatch the task.
fn post_process_task(file: &FileProgress, archive_path: &Path, install_path: &Path) -> Result<PostProcessTask, String> {
  use crate::handlers::dto::ManifestFileKind;
  let task = crate::handlers::dto::UnzipTask {
    file_name: file.name.clone(),
    archive_path: archive_path.to_path_buf(),
    destination_path: install_path.to_path_buf(),
  };
  match file.kind {
    ManifestFileKind::Raw => {
      // Target is a relative path inside the install dir ('/' separators).
      let rel = file
        .target
        .as_deref()
        .filter(|t| !t.is_empty())
        .unwrap_or(&file.name);
      let rel_norm = rel.replace('\\', "/");
      crate::utils::paths::assert_relative_target(&rel_norm)?;
      let dest = install_path.join(&rel_norm);
      Ok(PostProcessTask::Copy(crate::handlers::dto::UnzipTask {
        destination_path: dest,
        ..task
      }))
    }
    _ => Ok(PostProcessTask::Unzip(task)),
  }
}

/// Sync body of the raw-file placement. The download-dir copy is deliberately
/// LEFT IN PLACE: the caller records `is_unpacked` in the config first and only
/// then deletes the source, so a kill in between re-copies a file that is still
/// there instead of re-downloading it (same ordering as the zip branch, which
/// only removes the archive after the flag is persisted).
///
/// Same volume → a hard link costs nothing and keeps the source name alive;
/// everything else falls back to a real copy with a post-copy size check.
fn place_raw_file_sync(src: &Path, dest: &Path) -> std::io::Result<()> {
  if let Some(parent) = dest.parent() {
    std::fs::create_dir_all(parent)?;
  }

  // Both hard_link and a read-only destination would make the write fail, so
  // clear the way first (an install being repaired already has the file).
  if let Ok(meta) = std::fs::metadata(dest) {
    let mut perms = meta.permissions();
    if perms.readonly() {
      perms.set_readonly(false);
      let _ = std::fs::set_permissions(dest, perms);
    }
    let _ = std::fs::remove_file(dest);
  }

  if std::fs::hard_link(src, dest).is_ok() {
    return Ok(());
  }

  std::fs::copy(src, dest)?;
  let src_len = std::fs::metadata(src).map(|m| m.len()).unwrap_or(0);
  let dst_len = std::fs::metadata(dest).map(|m| m.len()).unwrap_or(u64::MAX);
  if src_len != dst_len {
    return Err(std::io::Error::other(format!("copy size mismatch: {} -> {}", src_len, dst_len)));
  }
  Ok(())
}

/// The post-process manager: consumes Unzip/Copy tasks in order. Replaces the
/// hand-copied unzip-manager loops of the start/continue commands; reports
/// unpack/copy failures to the UI instead of only logging them (plan P4/2.5).
/// `downloaded_cnt`/`total_file_count` are the version-level counters: a
/// terminal post-process failure gives the file's slot back (the download
/// worker counts a file as done as soon as its payload is verified on disk).
pub fn spawn_postprocess_manager(
  app: tauri::AppHandle,
  app_config: Arc<Mutex<AppConfig>>,
  service_unpack: Arc<ServiceUnpacker>,
  version_name: String,
  mut rx_unzip: mpsc::Receiver<PostProcessTask>,
  cancel: crate::handlers::start_download_version::CancelHandle,
  downloaded_cnt: Arc<std::sync::atomic::AtomicU32>,
  total_file_count: u32,
) -> tokio::task::JoinHandle<()> {
  tokio::spawn(async move {
    while let Some(task) = rx_unzip.recv().await {
      // Stop unpacking once the user cancelled: the frontend deletes the
      // install dir right after a cancel, and a manager that kept draining the
      // queue would either fight that deletion or re-create the directory with
      // a half-extracted archive.
      if cancel.is_cancelled() {
        log::info!("Post-process queue aborted by cancel");
        break;
      }
      match task {
        PostProcessTask::Unzip(data) => {
          let file_name = data.file_name.clone();
          let archive_path = data.archive_path.clone();
          let v_name = version_name.clone();
          let svc = service_unpack.clone();
          let v_name_thread = v_name.clone();

          // Unpacking is CPU-intensive → spawn_blocking returning whether it
          // succeeded, so the config update happens in the async context.
          let unpack_ok: bool = tokio::task::spawn_blocking(move || {
            let res = svc.extract_zip(&v_name_thread, &data.file_name, &data.archive_path, &data.destination_path);
            if let Err(e) = &res {
              log::error!("Unpack of '{}' failed: {}", &data.file_name, e);
            }
            res.is_ok()
          })
          .await
          .unwrap_or(false);

          if unpack_ok {
            let _ = app.emit("file-unzipped", (&v_name, archive_path.to_str()));
            let mut config_guard = app_config.lock().await;
            if let Some(ver) = config_guard.progress_download.get_mut(&v_name) {
              if let Some(file_progress) = ver.files.get_mut(&file_name) {
                file_progress.is_unpacked = true;
              }
            }
            let _ = config_guard.save();
            drop(config_guard);
            let _ = std::fs::remove_file(&archive_path);
          } else {
            // Hash already matched, so a broken archive means disk trouble or
            // a packer bug — re-downloading will not help. Keep the archive
            // for inspection and fail the version.
            {
              let mut config_guard = app_config.lock().await;
              if let Some(ver) = config_guard.progress_download.get_mut(&v_name) {
                if let Some(file_progress) = ver.files.get_mut(&file_name) {
                  file_progress.last_error = Some(FILE_ERR_UNPACK_FAILED.to_string());
                }
              }
              let _ = config_guard.save();
            }
            let _ = app.emit(
              "download-version-file-error",
              FileErrorPayload {
                version_name: v_name.clone(),
                file: file_name.clone(),
                code: FILE_ERR_UNPACK_FAILED.to_string(),
                message: format!("unpack failed, archive kept at {}", archive_path.display()),
              },
            );
            rollback_downloaded_count(&app, &app_config, &downloaded_cnt, &v_name, &file_name, total_file_count).await;
          }
        }
        PostProcessTask::Copy(data) => {
          let file_name = data.file_name.clone();
          let src = data.archive_path.clone();
          let dest = data.destination_path.clone();
          let v_name = version_name.clone();

          let src_c = src.clone();
          let copy_res = tokio::task::spawn_blocking(move || place_raw_file_sync(&src_c, &dest)).await.unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())));

          match copy_res {
            Ok(()) => {
              // `is_unpacked` semantics: "post-processed" (unzipped OR copied).
              // Persisted BEFORE the download-dir copy is dropped, exactly like
              // the zip branch deletes the archive only after the flag is saved:
              // a kill in between then finds the source still in place and just
              // re-copies it, instead of re-downloading the whole file.
              {
                let mut config_guard = app_config.lock().await;
                if let Some(ver) = config_guard.progress_download.get_mut(&v_name) {
                  if let Some(file_progress) = ver.files.get_mut(&file_name) {
                    file_progress.is_unpacked = true;
                  }
                }
                let _ = config_guard.save();
              }
              let _ = std::fs::remove_file(&src);
              // The frontend treats this event as "file is ready".
              let _ = app.emit("file-unzipped", (&v_name, data.archive_path.to_str()));
            }
            Err(e) => {
              log::error!("Copy of raw file '{}' failed: {}", &file_name, e);
              {
                let mut config_guard = app_config.lock().await;
                if let Some(ver) = config_guard.progress_download.get_mut(&v_name) {
                  if let Some(file_progress) = ver.files.get_mut(&file_name) {
                    file_progress.last_error = Some(FILE_ERR_COPY_FAILED.to_string());
                  }
                }
                let _ = config_guard.save();
              }
              let _ = app.emit(
                "download-version-file-error",
                FileErrorPayload {
                  version_name: v_name.clone(),
                  file: file_name.clone(),
                  code: FILE_ERR_COPY_FAILED.to_string(),
                  message: e.to_string(),
                },
              );
              rollback_downloaded_count(&app, &app_config, &downloaded_cnt, &v_name, &file_name, total_file_count).await;
            }
          }
        }
      }
    }
    log::info!("Post-process queue finished");
  })
}

/// Common tail of the start/continue/repair commands: waits for the post-process
/// queue to drain, then decides the version outcome.
/// - files with `last_error` → DownloadStatus::Error + Err(DOWNLOAD_FAILED),
///   no `download-unpack-version` event, progress stays in the config;
/// - all files done → is_downloaded = true + `download-unpack-version` event;
/// - anything else → Err(USER_CANCELLED) (pause semantics, as before).
pub async fn finalize_download(
  app: &tauri::AppHandle,
  app_config: &Arc<Mutex<AppConfig>>,
  version_name: &str,
  downloaded_total: u32,
  total_file_count: u32,
  unzip_manager: tokio::task::JoinHandle<()>,
  cancel: &crate::handlers::start_download_version::CancelHandle,
) -> Result<(), String> {
  let _ = unzip_manager.await;

  // `downloaded_total` counts files that finished DOWNLOADING. Post-processing
  // runs behind them, and the post-process manager bails out on cancel — so
  // when the player hits Pause while the last archives are still unpacking,
  // every file is "downloaded" and nothing looks wrong here. Declaring the
  // version installed at that point makes the frontend register it and delete
  // the download folder, leaving a game with half its gamedata missing and no
  // way to resume. Completion therefore means: not cancelled, and every file
  // actually post-processed.
  let (has_errors, downloaded_files_cnt, all_unpacked, tracked) = {
    let cfg = app_config.lock().await;
    match cfg.progress_download.get(version_name) {
      Some(ver) => (
        ver.files.values().any(|f| f.last_error.is_some()),
        ver.downloaded_files_cnt,
        // `Manifest` entries are metadata, not payload — they are never
        // post-processed, so requiring a flag on them would hang the version.
        !ver.files.is_empty()
          && ver
            .files
            .values()
            .filter(|f| f.kind != crate::handlers::dto::ManifestFileKind::Manifest)
            .all(|f| f.is_unpacked),
        true,
      ),
      // No progress entry left (e.g. the frontend cleared it): nothing we can
      // claim to have finished.
      None => (false, downloaded_total, false, false),
    }
  };
  let fully_downloaded = tracked && !cancel.is_cancelled() && downloaded_total >= total_file_count && all_unpacked;

  if has_errors {
    log::warn!(
      "Download of '{}' finished with errored files ({}), keeping progress, no unpack event",
      version_name,
      downloaded_files_cnt
    );
    let _ = app.emit(
      "download-version",
      DownloadProgress {
        version_name: version_name.to_string(),
        status: DownloadStatus::Error,
        file: String::new(),
        progress: if total_file_count > 0 { (downloaded_files_cnt as f32 / total_file_count as f32) * 100.0 } else { 0.0 },
        downloaded_files_cnt,
        total_file_count,
      },
    );
    return Err(ERR_DOWNLOAD_FAILED.to_string());
  }

  if fully_downloaded {
    {
      let mut config_guard = app_config.lock().await;
      if let Some(ver) = config_guard.progress_download.get_mut(version_name) {
        ver.is_downloaded = true;
      }
      let _ = config_guard.save();
    }

    let _ = app.emit("download-unpack-version", version_name);
    Ok(())
  } else {
    log::info!(
      "Download of '{}' did not complete (downloaded {}/{}); keeping progress, no unpack event",
      version_name,
      downloaded_total,
      total_file_count
    );
    Err(ERR_USER_CANCELLED.to_string())
  }
}

/// Build the shared worker context and run the pipeline. `version` must already
/// be inserted into `progress_download` by the caller.
pub async fn run_version_pipeline(
  app: &tauri::AppHandle,
  app_config: &Arc<Mutex<AppConfig>>,
  service_files: &Arc<ServiceFiles>,
  service_unpack: &Arc<ServiceUnpacker>,
  api_client: ApiClient,
  version: &VersionProgress,
  files_to_download: Vec<FileProgress>,
  files_to_postprocess: Vec<FileProgress>,
  cancel: crate::handlers::start_download_version::CancelHandle,
) -> Result<(), String> {
  // Completion counter base: files already downloaded/unpacked in earlier
  // runs are included in downloaded_files_cnt by the caller's scan.
  let total_file_count = version.total_file_count;
  let downloaded_cnt = Arc::new(std::sync::atomic::AtomicU32::new(version.downloaded_files_cnt));

  let shared = Arc::new(DownloadWorkerShared {
    app: app.clone(),
    app_config: app_config.clone(),
    service_files: service_files.clone(),
    api_client,
    version_name: version.name.clone(),
    download_dir: PathBuf::from(&version.download_path),
    install_path: PathBuf::from(&version.installed_path),
    total_file_count,
    downloaded_cnt: downloaded_cnt.clone(),
    cancel: cancel.clone(),
    last_save: std::sync::Mutex::new(std::time::Instant::now()),
  });

  // tokio's channel capacity must be > 0; an empty release would panic here.
  let (tx_queue, rx_queue) = mpsc::channel::<FileProgress>((total_file_count as usize).saturating_add(100).max(1));
  for file in files_to_download {
    let _ = tx_queue.send(file).await;
  }
  // Drop the queue sender BEFORE running the workers: they finish when the
  // queue drains and no sender remains (the local retry path does not re-send).
  drop(tx_queue);

  let (tx_unzip, rx_unzip) = mpsc::channel::<PostProcessTask>((total_file_count as usize).max(1));
  let unzip_manager = spawn_postprocess_manager(
    app.clone(),
    app_config.clone(),
    service_unpack.clone(),
    version.name.clone(),
    rx_unzip,
    cancel.clone(),
    downloaded_cnt.clone(),
    total_file_count,
  );

  // Files that were downloaded earlier (e.g. resume): queue their
  // post-processing before the workers start.
  for file in files_to_postprocess {
    let archive_path = match crate::utils::paths::safe_download_join(Path::new(&version.download_path), &file.name) {
      Ok(p) => p,
      Err(e) => {
        log::error!("safe_download_join failed: {}", e);
        mark_file_error_direct(app, app_config, &version.name, &file.name, FILE_ERR_COPY_FAILED, e.to_string()).await;
        continue;
      }
    };
    match post_process_task(&file, &archive_path, Path::new(&version.installed_path)) {
      Ok(task) => {
        let _ = tx_unzip.send(task).await;
      }
      Err(e) => {
        log::error!("Invalid raw target for '{}': {}", &file.name, &e);
        mark_file_error_direct(app, app_config, &version.name, &file.name, FILE_ERR_COPY_FAILED, e).await;
      }
    }
  }

  run_download_workers(shared, rx_queue, tx_unzip.clone()).await;

  // Drop the sender so the post-process manager drains and exits.
  drop(tx_unzip);

  let downloaded_total = downloaded_cnt.load(std::sync::atomic::Ordering::SeqCst);
  finalize_download(app, app_config, &version.name, downloaded_total, total_file_count, unzip_manager, &cancel).await
}

#[cfg(test)]
mod tests {
  use super::*;

  fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gw_dl_worker_{}_{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
  }

  // --- net_retries accounting (audit item 1) ---------------------------------

  #[test]
  fn net_retries_grow_only_while_nothing_is_downloaded() {
    // Five fruitless attempts in a row exhaust the budget…
    let mut retries = 0;
    for expected in 1..=(MAX_DOWNLOAD_RETRIES + 1) {
      retries = next_net_retries(retries, 1_000, 1_000);
      assert_eq!(retries, expected);
    }
    assert!(retries > MAX_DOWNLOAD_RETRIES, "budget must be exhausted by consecutive failures");
  }

  #[test]
  fn net_retries_reset_when_the_attempt_moved_the_resume_point() {
    // A 2 GiB file that drops the connection every few hundred megabytes must
    // never hit the terminal NETWORK error: every attempt that appended bytes
    // starts the count from scratch.
    let mut retries = 0;
    let mut offset: u64 = 0;
    for _ in 0..50 {
      let after = offset + 300 * 1024 * 1024;
      retries = next_net_retries(retries, offset, after);
      assert_eq!(retries, 0);
      assert!(retries <= MAX_DOWNLOAD_RETRIES);
      offset = after;
    }
  }

  #[test]
  fn net_retries_reset_even_for_a_single_new_byte() {
    assert_eq!(next_net_retries(MAX_DOWNLOAD_RETRIES, 100, 101), 0);
    // A stalled attempt (or one that restarted from zero) still counts.
    assert_eq!(next_net_retries(3, 100, 100), 4);
    assert_eq!(next_net_retries(3, 100, 0), 4);
  }

  #[tokio::test]
  async fn read_part_offset_reads_the_sidecar_and_defaults_to_zero() {
    let dir = temp_dir("part");
    let part = dir.join("f.zip.part");
    assert_eq!(read_part_offset(part.to_str().unwrap()).await, 0);
    std::fs::write(&part, b"12345\n").unwrap();
    assert_eq!(read_part_offset(part.to_str().unwrap()).await, 12345);
    std::fs::write(&part, b"garbage").unwrap();
    assert_eq!(read_part_offset(part.to_str().unwrap()).await, 0);
    let _ = std::fs::remove_dir_all(&dir);
  }

  // --- verify (audit item 3) -------------------------------------------------

  #[tokio::test]
  async fn verify_rejects_a_zero_size_manifest_entry() {
    let dir = temp_dir("verify_zero");
    let file = dir.join("empty.bin");
    std::fs::write(&file, b"").unwrap();

    // An empty file used to "match" an expected size of 0 and be counted as
    // downloaded; a zero-size entry is a broken manifest instead.
    let res = verify_downloaded_file(&file, 0, None).await;
    assert!(res.is_err(), "expected_size == 0 must not verify, got {:?}", res.ok());

    let _ = std::fs::remove_dir_all(&dir);
  }

  #[tokio::test]
  async fn verify_still_accepts_a_real_file_without_sha256() {
    let dir = temp_dir("verify_ok");
    let file = dir.join("payload.bin");
    std::fs::write(&file, b"0123456789").unwrap();

    match verify_downloaded_file(&file, 10, None).await.unwrap() {
      VerifyResult::Skipped => {}
      other => panic!("expected Skipped, got {:?}", other),
    }
    match verify_downloaded_file(&file, 11, None).await.unwrap() {
      VerifyResult::SizeMismatch { expected: 11, actual: 10 } => {}
      other => panic!("expected SizeMismatch, got {:?}", other),
    }

    let _ = std::fs::remove_dir_all(&dir);
  }

  // --- raw file placement (audit item 4) -------------------------------------

  #[test]
  fn place_raw_file_keeps_the_source_until_the_caller_drops_it() {
    let dir = temp_dir("raw");
    let src = dir.join("download/patch.exe");
    std::fs::create_dir_all(src.parent().unwrap()).unwrap();
    std::fs::write(&src, b"payload").unwrap();
    let dest = dir.join("install/bin/patch.exe");

    place_raw_file_sync(&src, &dest).unwrap();

    // The source must survive the call: `is_unpacked = true` is persisted first
    // and only then is the download-dir copy removed, so a kill in between
    // re-copies instead of re-downloading.
    assert!(src.exists(), "source must still be there after placement");
    assert_eq!(std::fs::read(&dest).unwrap(), b"payload");

    // Overwriting an existing destination keeps working (repair/reinstall).
    std::fs::write(&src, b"payload2").unwrap();
    place_raw_file_sync(&src, &dest).unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"payload2");
    assert!(src.exists());

    let _ = std::fs::remove_dir_all(&dir);
  }

  // `persist_file_size` (audit item 2) is not unit-tested: building an
  // `AppConfig` pulls in the tauri runtime, which cannot be constructed off the
  // main thread. Its guard is the `part_path.is_empty() || part_path == ".part"`
  // early return plus the call sites, which no longer build a path they could
  // not resolve.
}
