use crate::{
  configs::AppConfig::{AppConfig, FileProgress},
  consts,
  handlers::{
    dto::{DownlaodFileStat, DownloadProgress, DownloadStatus},
    start_download_version::CancelMap,
  },
  service::{download_worker::VerifyResult, files::ServiceFiles, main::Service, unpack::ServiceUnpacker},
};
use std::{cmp::Reverse, path::{Path, PathBuf}, sync::Arc};
use tauri::Emitter;
use tokio::sync::{Mutex, broadcast};

#[tauri::command]
pub async fn continue_download_version(
  app: tauri::AppHandle,
  channel_map: tauri::State<'_, CancelMap>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  service_files: tauri::State<'_, Arc<ServiceFiles>>,
  service_unpack: tauri::State<'_, Arc<ServiceUnpacker>>,
  versionName: String,
) -> Result<(), String> {
  log::info!("Start continue_download_version, version: {:?}", &versionName);

  if crate::utils::locks::lock(&channel_map).contains_key(&versionName) {
    return Err(consts::ERR_DOWNLOAD_ALREADY_RUNNING.to_string());
  }

  // 1. Инициализация каналов отмены
  let (cancel_tx, _) = broadcast::channel::<()>(1);
  {
    crate::utils::locks::lock(&channel_map).insert(versionName.clone(), cancel_tx.clone());
  }
  scopeguard::defer! {
    crate::utils::locks::lock(&channel_map).remove(&versionName);
  };

  // 2. Сбор статистики и подготовка данных
  let mut file_sizes: Vec<DownlaodFileStat> = vec![];

  // Files that finished downloading in an earlier run and only need a resume
  // re-verify (P5/2.3): the sha256 hash is computed OUTSIDE the config lock
  // below — hashing a multi-GB part while holding the global config mutex
  // would stall every other command that reads the config (bug fix).
  struct PendingVerify {
    name: String,
    file_path: PathBuf,
    part_path: PathBuf,
    total_size: u64,
    sha256: Option<String>,
    file_progress: FileProgress,
  }
  let mut pending_verify: Vec<PendingVerify> = vec![];

  let (mut version, mut files_to_download, mut files_to_postprocess) = {
    let mut cfg_guard = app_config.lock().await;

    // Ensure the download dir exists on resume: start_download_version creates it,
    // but continue_download_version previously did NOT, so a post-restart resume
    // could finish "successfully" with no real files on disk, then fail downstream
    // when removing an already-absent download dir (os error 3).
    {
      let version_data = cfg_guard
        .progress_download
        .get(&versionName)
        .ok_or_else(|| "Version not found".to_string())?;
      let download_dir = Path::new(&version_data.download_path);
      if !download_dir.exists() {
        std::fs::create_dir_all(download_dir).map_err(|e| e.to_string())?;
      }
    }

    let mut to_download = Vec::new();
    let mut to_postprocess = Vec::new();
    let version_data = {
      let version_data = cfg_guard
        .progress_download
        .get_mut(&versionName)
        .ok_or_else(|| "Version not found".to_string())?;

      let mut files_dwn_cnt: u32 = 0;

      for (_, file_progress) in version_data.files.iter_mut() {
        let file_path = Path::new(&version_data.download_path).join(&file_progress.name);
        let file_part_path = Path::new(&version_data.download_path).join(format!("{}.part", &file_progress.name));

        // ---------------------------------------------------------------
        // P5: the file on disk is the source of truth, the `.part` sidecar
        // is only a hint written before the file was flushed.
        // ---------------------------------------------------------------
        let part_size: Option<u64> = tokio::fs::read_to_string(&file_part_path)
          .await
          .ok()
          .and_then(|s| s.trim().parse::<u64>().ok());
        let file_len: u64 = match tokio::fs::metadata(&file_path).await {
          Ok(meta) => meta.len(),
          Err(_) => 0,
        };

        let current_size = if let Some(part) = part_size {
          if file_len == 0 {
            // Sidecar exists but the file is gone → start over.
            0
          } else if file_len < part {
            // Crash lost bytes after the sidecar write → trust the file.
            file_len
          } else if file_len > part {
            // Extra bytes of unknown quality past the recorded point → cut
            // the file back so the Range resume stays consistent.
            let f = std::fs::OpenOptions::new().write(true).open(&file_path);
            if let Ok(f) = f {
              let _ = f.set_len(part);
            }
            part
          } else {
            part
          }
        } else if file_len > 0 && file_progress.is_downloaded {
          file_len
        } else if file_len == 0 && file_progress.is_unpacked {
          // Already post-processed: the archive was removed after unpack.
          file_progress.total_size
        } else {
          0
        };

        file_progress.size = current_size;

        // ---------------------------------------------------------------
        // Retry entries from a previous failed run: reset the counters and
        // re-route by error kind (network/hash → download, unpack/copy →
        // post-process; the archive is still on disk for the latter).
        // ---------------------------------------------------------------
        if let Some(err) = file_progress.last_error.take() {
          file_progress.net_retries = 0;
          file_progress.verify_retries = 0;
          match err.as_str() {
            consts::FILE_ERR_UNPACK_FAILED | consts::FILE_ERR_COPY_FAILED => {
              if file_len > 0 {
                file_progress.is_downloaded = true;
                to_postprocess.push(file_progress.clone());
                files_dwn_cnt += 1;
              } else {
                file_progress.is_downloaded = false;
                to_download.push(file_progress.clone());
              }
            }
            _ => {
              file_progress.is_downloaded = false;
              to_download.push(file_progress.clone());
            }
          }

          file_sizes.push(DownlaodFileStat {
            name: file_progress.name.clone(),
            unpacked: file_progress.is_unpacked,
            size: Some(current_size),
          });
          continue;
        }

        if current_size >= file_progress.total_size && file_progress.total_size > 0 {
          file_progress.is_downloaded = true;
          files_dwn_cnt += 1;

          // 2.3: a file marked downloaded but not yet post-processed is
          // re-verified before unpacking — it may have been corrupted on
          // disk between sessions. On mismatch it goes back to the queue.
          // The hash itself is computed AFTER the config lock is released
          // (see `pending_verify` below) — this loop only queues it.
          if !file_progress.is_unpacked && file_len > 0 {
            pending_verify.push(PendingVerify {
              name: file_progress.name.clone(),
              file_path: file_path.clone(),
              part_path: file_part_path.clone(),
              total_size: file_progress.total_size,
              sha256: file_progress.sha256.clone(),
              file_progress: file_progress.clone(),
            });
          }
        } else {
          file_progress.is_downloaded = false;
          to_download.push(file_progress.clone());
        }

        file_sizes.push(DownlaodFileStat {
          name: file_progress.name.clone(),
          unpacked: file_progress.is_unpacked,
          size: Some(current_size),
        });
      }

      version_data.downloaded_files_cnt = files_dwn_cnt;
      version_data.clone()
    };

    cfg_guard.save().map_err(|e| e.to_string())?;

    (version_data.clone(), to_download, to_postprocess)
  };
  // ^ config lock released here — the hashing below must not hold it.

  // Phase B: resume re-verify (P5/2.3), OUTSIDE the config lock.
  let mut regressed_names: Vec<String> = vec![];
  for pv in pending_verify {
    let verify = crate::service::download_worker::verify_downloaded_file(&pv.file_path, pv.total_size, pv.sha256.as_deref()).await;
    match verify {
      Ok(VerifyResult::Ok) | Ok(VerifyResult::Skipped) => {
        files_to_postprocess.push(pv.file_progress);
      }
      Ok(VerifyResult::SizeMismatch { .. }) | Ok(VerifyResult::HashMismatch { .. }) => {
        log::warn!("Resume verify failed for '{}': re-downloading", &pv.name);
        let _ = tokio::fs::remove_file(&pv.file_path).await;
        let _ = tokio::fs::remove_file(&pv.part_path).await;
        let mut fp = pv.file_progress;
        fp.is_downloaded = false;
        fp.size = 0;
        regressed_names.push(fp.name.clone());
        files_to_download.push(fp);
      }
      Err(e) => {
        // I/O error while hashing (AV lock, transient disk issue): the size
        // already matched, so this is not necessarily corruption. Keep the
        // fully downloaded file instead of discarding it — let it through to
        // post-processing rather than forcing a redundant re-download.
        log::warn!("Resume verify of '{}' errored (kept, not re-downloaded): {}", &pv.name, e);
        files_to_postprocess.push(pv.file_progress);
      }
    }
  }

  if !regressed_names.is_empty() {
    version.downloaded_files_cnt = version.downloaded_files_cnt.saturating_sub(regressed_names.len() as u32);
    let mut cfg_guard = app_config.lock().await;
    if let Some(ver) = cfg_guard.progress_download.get_mut(&versionName) {
      ver.downloaded_files_cnt = version.downloaded_files_cnt;
      for name in &regressed_names {
        if let Some(existing) = ver.files.get_mut(name) {
          existing.is_downloaded = false;
          existing.size = 0;
        }
      }
    }
    let _ = cfg_guard.save();
  }

  // Сортировка для UI (по номеру чанка в расширении)
  file_sizes.sort_by_key(|file| Reverse(file.size));
  files_to_download.sort_by_key(|file| Reverse(file.size));

  let _ = app.emit("download-version-files", (&versionName, &file_sizes));
  // Bug E fix: guard against division by zero.
  let progress = if version.total_file_count > 0 {
    (version.downloaded_files_cnt as f32 / version.total_file_count as f32) * 100.0
  } else {
    0.0
  };
  // Отправляем ивент на фронт с сохраненными данными о прогресса после паузы. Это нужна для начальной инициализации UI
  let _ = app.emit(
    "download-version",
    DownloadProgress {
      version_name: version.name.clone(),
      status: DownloadStatus::DownloadFiles,
      file: "".to_string(),
      progress,
      downloaded_files_cnt: version.downloaded_files_cnt,
      total_file_count: version.total_file_count,
    },
  );

  let api_client = service.lock().await.api_client.clone();

  crate::service::download_worker::run_version_pipeline(
    &app,
    &app_config.inner().clone(),
    &service_files.inner().clone(),
    &service_unpack.inner().clone(),
    api_client,
    &version,
    files_to_download,
    files_to_postprocess,
    cancel_tx,
  )
  .await
}
