use crate::{
  configs::AppConfig::{AppConfig, FileProgress, VersionProgress},
  consts::PULL_FILES_SIZE,
  handlers::dto::{DownlaodFileStat, DownloadProgress, DownloadStatus, UnzipTask},
  service::{
    files::{DownloadOutcome, ServiceFiles},
    get_release::ServiceGetRelease,
    main::Service,
    unpack::ServiceUnpacker,
  },
  utils::errors::log_full_error,
};

/// Max download attempts per file before giving up.
const MAX_DOWNLOAD_RETRIES: u32 = 5;
use anyhow::Context;
use std::{
  collections::HashMap,
  fs,
  path::{Path, PathBuf},
  sync::{Arc, Mutex as StdMutex},
};
use std::{
  sync::atomic::{AtomicU32, Ordering},
  time::Duration,
};
use tauri::Emitter;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, broadcast};

pub type CancelMap = Arc<StdMutex<HashMap<String, broadcast::Sender<()>>>>;

#[tauri::command]
pub async fn cancel_download_version(channel_map: tauri::State<'_, CancelMap>, releaseName: String) -> Result<(), String> {
  if let Some(tx) = crate::utils::locks::lock(&channel_map).remove(&releaseName) {
    let _ = tx.send(());
  }

  Ok(())
}

/// Cancels every active download, persists the config, then returns.
/// Used as a graceful-shutdown hook on window close / app exit so that partial
/// progress is saved instead of being lost by `process::exit`.
#[tauri::command]
pub async fn cancel_all_downloads_and_save(
  channel_map: tauri::State<'_, CancelMap>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<(), String> {
  let senders: Vec<broadcast::Sender<()>> = {
    let map = crate::utils::locks::lock(&channel_map);
    map.iter().map(|(_, v)| v.clone()).collect()
  };

  // Signal every active download worker to stop (they persist .part + config on the way out).
  for tx in senders {
    let _ = tx.send(());
  }

  // Give workers a brief moment to flush their .part files and config updates.
  tokio::time::sleep(Duration::from_millis(500)).await;

  // Final defensive save of the whole config.
  let mut config_guard = app_config.lock().await;
  let _ = config_guard.save();
  Ok(())
}

#[tauri::command]
pub async fn start_download_version(
  app: tauri::AppHandle,
  channel_map: tauri::State<'_, CancelMap>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  service_files: tauri::State<'_, Arc<ServiceFiles>>,
  service_unpack: tauri::State<'_, Arc<ServiceUnpacker>>,
  downloadPath: String,
  installPath: String,
  versionName: String,
  versionId: Option<u32>,
) -> Result<(), String> {
  // Guard before insert so a second start cannot orphan the first cancel channel.
  if crate::utils::locks::lock(&channel_map).contains_key(&versionName) {
    return Err("DOWNLOAD_ALREADY_RUNNING".to_string());
  }

  // Bug C fix: single broadcast channel for the whole command. Previously there
  // were two disconnected channels: `tx`/`rx` (registered first) and `cancel_tx`
  // (created later and used by workers). Cancellation sent via `cancel_tx` never
  // reached `rx`, so the early cancel checks were dead. Now one `cancel_tx` is
  // created upfront, registered in the map, used for early checks AND subscribed
  // to by the workers.
  let (cancel_tx, mut rx) = broadcast::channel::<()>(1);
  {
    crate::utils::locks::lock(&channel_map).insert(versionName.clone(), cancel_tx.clone());
  }
  // Удаляем запись после завершения (успешного или нет)
  scopeguard::defer! {
    crate::utils::locks::lock(&channel_map).remove(&versionName);
  };

  let cfg = app_config.lock().await.clone();

  let selected_version = cfg
    .versions
    .iter()
    .find(|v| {
      if v.name == versionName {
        return true;
      }
      if let Some(id) = versionId {
        return v.id == id;
      }

      return false;
    })
    .ok_or_else(|| anyhow::anyhow!("Version not found, versionName: {:?} versionId: {:?}", &versionName, &versionId))
    .map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

  log::info!("start_download_versions, selected_version: {:?}", &selected_version);

  let mut total_file_count: u32 = 0;

  if let Some(data) = &selected_version.manifest {
    total_file_count = data.files.len() as u32;
  };

  let mut version = VersionProgress {
    id: selected_version.id,
    name: selected_version.name.clone(),
    path: selected_version.path.clone(),
    installed_path: installPath.clone(),
    download_path: downloadPath.clone(),
    is_downloaded: false,
    files: HashMap::new(),
    downloaded_files_cnt: 0,
    total_file_count,
    manifest: selected_version.manifest.clone(),
  };

  let _ = app.emit(
    "download-version",
    DownloadProgress {
      version_name: version.name.clone(),
      status: DownloadStatus::Init,
      file: "".to_owned(),
      progress: 0.0,
      downloaded_files_cnt: 0,
      total_file_count: version.total_file_count,
    },
  );

  let download_dir = Path::new(&version.download_path);
  std::fs::create_dir_all(&download_dir)
    .with_context(|| format!("Failed to create output download directory: {:?}", download_dir))
    .map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

  let release = {
    let service_guard = service.lock().await;
    service_guard
      .get_main_release(&selected_version.name)
      .await
      .context("Failed to get main release files")
      .map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?
  };

  if rx.try_recv().is_ok() {
    log::info!("Download task '{}' was cancelled", &versionName);
    return Err("USER_CANCELLED".to_string());
  }

  version.total_file_count = release.assets.len() as u32;

  let _ = app.emit(
    "download-version",
    DownloadProgress {
      version_name: version.name.clone(),
      status: DownloadStatus::Init,
      file: "".to_owned(),
      progress: 0.0,
      downloaded_files_cnt: version.downloaded_files_cnt,
      total_file_count: version.total_file_count,
    },
  );

  for file in &release.assets {
    version.files.insert(
      file.name.clone(),
      FileProgress {
        id: file.name.clone(),
        download_link: file.download_link.clone(),
        name: file.name.clone(),
        is_downloaded: false,
        is_unpacked: false,
        size: 0,
        total_size: file.size,
      },
    );
  }

  if rx.try_recv().is_ok() {
    log::info!("Download task '{}' was cancelled", &versionName);
    return Err("USER_CANCELLED".to_string());
  }

  {
    let mut config_guard = app_config.lock().await;
    config_guard.progress_download.insert(version.name.clone(), version.clone());
    config_guard.save().map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;
  }

  let mut file_sizes: Vec<DownlaodFileStat> = version
    .files
    .values()
    .map(|f| DownlaodFileStat {
      name: f.name.clone(),
      unpacked: false,
      size: Some(0),
    })
    .collect();
  file_sizes.sort_by(|a, b| a.name.cmp(&b.name));
  let _ = app.emit("download-version-files", (&versionName, &file_sizes));

  let _ = app.emit(
    "download-version",
    DownloadProgress {
      version_name: version.name.clone(),
      status: DownloadStatus::DownloadFiles,
      file: "".to_owned(),
      progress: 0.0,
      downloaded_files_cnt: version.downloaded_files_cnt,
      total_file_count: version.total_file_count,
    },
  );

  let total_file_count = release.assets.len() as u32;
  let downloaded_cnt = Arc::new(AtomicU32::new(0));

  // Создаем канал для очереди задач
  // Запас емкости берем с запасом, чтобы влезли все файлы + возможные ретраи
  let (tx_queue, mut rx_queue) = mpsc::channel(total_file_count as usize + 100);

  // Заполняем очередь начальными файлами
  for file in release.assets {
    tx_queue.send(file).await.map_err(|e| e.to_string())?;
  }

  // Обертка для доступа к API клиенту
  let api_client = {
    let service_guard = service.lock().await;
    service_guard.api_client.clone()
  };

  let (tx_unzip, mut rx_unzip) = mpsc::channel::<UnzipTask>(total_file_count as usize);

  // Отдельный поток-менеджер распаковки
  let app_unzip = app.clone();
  let version_name_unzip = versionName.clone();
  let service_unpack_arc = service_unpack.inner().clone();
  let app_config_arc = app_config.inner().clone();
  let unzip_manager_handle = tokio::spawn(async move {
    while let Some(data) = rx_unzip.recv().await {
      log::debug!("Worker got msg to unpack file, data: {:?}", &data);

      let app_inner = app_unzip.clone();
      let v_name = version_name_unzip.clone();
      let service_unpack_for_thread = service_unpack_arc.clone();
      let app_config_arc_for_thread = app_config_arc.clone();
      let archive_path = data.archive_path.clone();
      let file_name = data.file_name.clone();
      let v_name_for_thread = v_name.clone();

      // Unpacking is CPU-intensive → run it in spawn_blocking, returning whether it
      // succeeded so the config update happens in the async context (no block_on
      // inside a blocking thread, which previously risked starving the pool).
      let unpack_ok: bool = tokio::task::spawn_blocking(move || {
        let res = service_unpack_for_thread.extract_zip(&v_name_for_thread, &data.file_name, &data.archive_path, &data.destination_path);
        if let Err(e) = &res {
          log::error!("Unpack of '{}' failed: {}", &data.file_name, e);
        }
        let _ = app_inner.emit("file-unzipped", (&v_name_for_thread, data.archive_path.to_str()));
        res.is_ok()
      })
      .await
      .unwrap_or(false);

      // Config update + archive removal back in the async context.
      if unpack_ok {
        let mut config_guard = app_config_arc_for_thread.lock().await;
        if let Some(ver) = config_guard.progress_download.get_mut(&v_name) {
          if let Some(file_progress) = ver.files.get_mut(&file_name) {
            file_progress.is_unpacked = true;
          }
        }
        let _ = config_guard.save();
        drop(config_guard);
        let _ = fs::remove_file(&archive_path);
      }
    }
    log::info!("Unzip queue finished");
  });

  let mut join_handles = Vec::new();
  // Bug C fix: reuse the single `cancel_tx` created at the top of the command.
  // It is already registered in `channel_map`, so no second insert is needed.

  let rx_queue_arc = Arc::new(Mutex::new(rx_queue));
  let tx_queue_arc = Arc::new(Mutex::new(tx_queue));
  let tx_unzip_arc = Arc::new(tx_unzip);
  let cancel_tx_arc = Arc::new(cancel_tx);

  // Run a fixed number of download workers.
  for _ in 0..PULL_FILES_SIZE {
    let app_c = app.clone();
    let app_config_c = app_config.inner().clone();
    let service_files_c = service_files.inner().clone();
    let api_client_c = api_client.clone();
    let version_name_c = versionName.clone();
    let version_install_path_c = version.installed_path.clone();
    let download_dir_c = download_dir.to_path_buf();
    let downloaded_cnt_c = downloaded_cnt.clone();

    let tx_unzip_c = tx_unzip_arc.clone();
    let rx_queue_c = rx_queue_arc.clone();
    let cancel_tx_arc_c = cancel_tx_arc.clone();
    let mut stop_rx = cancel_tx_arc.subscribe();

    // Variable for rx queue (needs Mutex, as mpsc::Receiver is not Thread-safe)
    // But in this case we just pass ownership of rx to each worker via Arc/Mutex
    // or use a single-loop approach.
    let handle = tokio::spawn(async move {
      // Per-file retry counter so a persistently failing file does not loop forever.
      let mut retries: u32 = 0;
      let mut current_task: Option<_> = None;

      loop {
        // Take next task either from the previous failed attempt or from the queue.
        let file_task = if let Some(t) = current_task.take() {
          t
        } else {
          let mut rx_lock = rx_queue_c.lock().await;

          tokio::select! {
              // Stop if cancellation arrived
              _ = stop_rx.recv() => break,
              // Queue is empty, worker finishes
              task = rx_lock.recv() => {
                  match task {
                      Some(t) => t,
                      None => break,
                  }
              }
          }
        };

        let file_path = match crate::utils::paths::safe_download_join(&download_dir_c, &file_task.name) {
          Ok(p) => p,
          Err(e) => {
            log::error!("safe_download_join failed: {}", e);
            continue;
          }
        };
        let part_path = format!("{}.part", file_path.to_str().unwrap_or(""));

        // Read existing progress for Range header
        let seek_pos = if let Ok(content) = std::fs::read_to_string(&part_path) {
          content.trim().parse::<u64>().ok()
        } else {
          None
        };

        let mut local_cancel = cancel_tx_arc_c.subscribe();
        let res = service_files_c
          .download_blob_to_file(
            &api_client_c,
            &version_name_c,
            &file_task.download_link,
            &file_task.size,
            &file_path,
            &seek_pos,
            local_cancel,
          )
          .await;

        match res {
          Ok(DownloadOutcome::Completed) => {
            // Successfully downloaded
            retries = 0;
            let current = downloaded_cnt_c.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = tx_unzip_c
              .send(UnzipTask {
                file_name: file_task.name.clone(),
                archive_path: file_path.clone(),
                destination_path: PathBuf::from(&version_install_path_c),
              })
              .await;

            update_config_and_emit(&app_c, &app_config_c, &version_name_c, &file_task.name, current, total_file_count).await;
            if current >= total_file_count {
              let _ = cancel_tx_arc_c.send(());
              break;
            }
          }
          Ok(DownloadOutcome::Interrupted) => {
            // User pause / shutdown: persist partial progress to config and stop without
            // counting this file as completed.
            log::info!("Download of '{}' interrupted by cancel signal, saving progress", file_task.name);
            persist_file_size(&app_config_c, &version_name_c, &file_task.name, &part_path).await;
            break;
          }
          Err(e) => {
            retries += 1;
            if retries > MAX_DOWNLOAD_RETRIES {
              log::error!("Download of '{}' failed after {} attempts: {}", file_task.name, MAX_DOWNLOAD_RETRIES, e);
              persist_file_size(&app_config_c, &version_name_c, &file_task.name, &part_path).await;
              break;
            }
            log::warn!("Error downloading '{}' (attempt {}/{}): {}. Retrying...", file_task.name, retries, MAX_DOWNLOAD_RETRIES, e);
            // Persist partial size before retry so a subsequent kill keeps the resume point.
            persist_file_size(&app_config_c, &version_name_c, &file_task.name, &part_path).await;
            // Re-queue the same file for another attempt.
            current_task = Some(file_task);
            tokio::time::sleep(Duration::from_secs(2)).await; // pause before retry
          }
        }
      }
    });
    join_handles.push(handle);
  }

  // Важно: чтобы rx_queue закрылся, нужно дропнуть все tx_queue, кроме тех что в воркерах
  drop(tx_queue_arc);

  for h in join_handles {
    let _ = h.await;
  }

  // Determine whether the download completed fully. We only emit the completion
  // event when every file finished; otherwise the frontend would start unpacking
  // a partial download and wipe the saved progress.
  let downloaded_total = downloaded_cnt.load(Ordering::SeqCst);
  let fully_downloaded = downloaded_total >= total_file_count;

  // ВАЖНО: Закрываем передатчик очереди распаковки.
  // После этого rx_unzip.recv() вернет None, когда обработает ВСЕ задачи в очереди.
  drop(tx_unzip_arc);

  // Ждем, пока менеджер распаковки закончит последний файл
  let _ = unzip_manager_handle.await;

  if fully_downloaded {
    // Bug B fix: mark the version as fully downloaded in config.
    {
      let mut config_guard = app_config.lock().await;
      if let Some(ver) = config_guard.progress_download.get_mut(&versionName) {
        ver.is_downloaded = true;
      }
      let _ = config_guard.save();
    }

    let _ = app.emit("download-unpack-version", &versionName);
    Ok(())
  } else {
    log::info!(
      "Download of '{}' did not complete (downloaded {}/{}); keeping progress, no unpack event",
      &versionName,
      downloaded_total,
      total_file_count
    );
    Err("USER_CANCELLED".to_string())
  }
}

// Вспомогательная функция для получения задач из очереди внутри select!
async fn update_config_and_emit(
  app: &tauri::AppHandle,
  config: &Arc<Mutex<AppConfig>>,
  version_name: &str,
  file_name: &str,
  current: u32,
  total: u32,
) {
  let mut config_guard = config.lock().await;
  if let Some(ver) = config_guard.progress_download.get_mut(version_name) {
    if let Some(file_progress) = ver.files.get_mut(file_name) {
      file_progress.is_downloaded = true;
    }
    ver.downloaded_files_cnt = current;
  }
  let _ = config_guard.save();

  // Bug E fix: guard against division by zero when total is 0 (e.g. empty manifest).
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

/// Reads the `.part` sidecar and persists its byte count into `FileProgress.size`,
/// so the resume point survives an abrupt process kill. Called after interruptions/retries.
async fn persist_file_size(config: &Arc<Mutex<AppConfig>>, version_name: &str, file_name: &str, part_path: &str) {
  let size = match std::fs::read_to_string(part_path) {
    Ok(s) => s.trim().parse::<u64>().unwrap_or(0),
    Err(_) => 0,
  };

  let mut config_guard = config.lock().await;
  if let Some(ver) = config_guard.progress_download.get_mut(version_name) {
    if let Some(fp) = ver.files.get_mut(file_name) {
      fp.size = size;
    }
  }
  let _ = config_guard.save();
}
