use crate::{
  configs::AppConfig::{AppConfig, FileProgress, VersionProgress},
  consts::PULL_FILES_SIZE,
  handlers::dto::{DownloadProgress, DownloadStatus},
  service::{files::ServiceFiles, get_release::ServiceGetRelease, main::Service},
  utils::errors::log_full_error,
};
use anyhow::Context;
use std::sync::atomic::{AtomicU16, Ordering};
use std::{
  collections::HashMap,
  path::Path,
  sync::{Arc, Mutex as StdMutex},
};
use tauri::Emitter;
use tokio::sync::Semaphore;
use tokio::sync::{Mutex, broadcast};

pub type CancelMap = Arc<StdMutex<HashMap<String, broadcast::Sender<()>>>>;

#[tauri::command]
pub async fn cancel_download_version(channel_map: tauri::State<'_, CancelMap>, releaseName: String) -> Result<(), String> {
  if let Some(tx) = channel_map.lock().unwrap().remove(&releaseName) {
    let _ = tx.send(());
  }

  Ok(())
}

#[tauri::command]
pub async fn start_download_version(
  app: tauri::AppHandle,
  channel_map: tauri::State<'_, CancelMap>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  service_files: tauri::State<'_, Arc<ServiceFiles>>,
  downloadPath: String,
  installPath: String,
  versionName: String,
  versionId: Option<u32>,
) -> Result<(), String> {
  let (tx, mut rx) = broadcast::channel::<()>(1);
  {
    channel_map.lock().unwrap().insert(versionName.clone(), tx.clone());
  }
  // Удаляем запись после завершения (успешного или нет)
  scopeguard::defer! {
    channel_map.lock().unwrap().remove(&versionName);
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

  let mut version = VersionProgress {
    id: selected_version.id,
    name: selected_version.name.clone(),
    path: selected_version.path.clone(),
    installed_path: installPath.clone(),
    download_path: downloadPath.clone(),
    is_downloaded: false,
    files: HashMap::new(),
    downloaded_files_cnt: 0,
    total_file_count: 0,
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

  let all_files = {
    let service_guard = service.lock().await;
    service_guard
      .get_main_release_files(&selected_version.name)
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

  let mut files = Vec::new();

  for file in all_files {
    if !file.name.starts_with("game.7z") {
      continue;
    }

    files.push(file);
  }

  if rx.try_recv().is_ok() {
    log::info!("Download task '{}' was cancelled", &versionName);
    return Err("USER_CANCELLED".to_string());
  }

  version.total_file_count = files.len() as u16;

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

  for file in &files {
    let mut size: u64 = 0;
    if let Some(meta) = &selected_version.manifest {
      if let Some(found) = meta.files.iter().find(|f| f.name == file.name) {
        size = found.size.clone();
      };
    };
    version.files.insert(
      file.id.clone(),
      FileProgress {
        id: file.id.clone(),
        project_id: file.project_id.clone(),
        name: file.name.clone(),
        path: file.path.clone(),
        is_downloaded: false,
        size: 0,
        total_size: size,
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

  if rx.try_recv().is_ok() {
    log::info!("Download task '{}' was cancelled", &versionName);
    return Err("USER_CANCELLED".to_string());
  }

  let total_file_count = files.len() as u16;
  let downloaded_cnt = Arc::new(AtomicU16::new(0));
  let semaphore = Arc::new(Semaphore::new(PULL_FILES_SIZE as usize));

  let mut join_handles = Vec::new();

  for file in files {
    // 1. Проверяем, не была ли отменена загрузка перед стартом следующего файла
    if !rx.is_empty() || rx.try_recv().is_ok() {
      log::info!("Main loop caught cancel signal");
      let _ = app.emit("cancel-download-version", &versionName);
      break;
    }

    // 2. Ждем свободного места в пуле (макс 6)
    let permit = semaphore.clone().acquire_owned().await.map_err(|e| e.to_string())?;

    // 3. Логика ожидания 1 секунду перед добавлением следующего файла
    // tokio::time::sleep(Duration::from_secs(1)).await;

    // Клонируем всё необходимое для перемещения в async block
    let app_c = app.clone();
    let app_config_c = app_config.inner().clone();
    let service_files_c = service_files.inner().clone();
    let service_c = service.inner().clone();
    let version_name_c = versionName.clone();
    let file_c = file.clone();
    let download_dir_c = download_dir.to_path_buf();
    let downloaded_cnt_c = downloaded_cnt.clone();

    let mut local_rx = tx.subscribe();
    let local_2_rx = tx.subscribe();

    let handle = tokio::spawn(async move {
      // Переменная для контроля разрешения (permit)
      // Она будет удерживаться до конца выполнения этого блока
      let _permit = permit;

      // Проверка отмены перед самым началом скачивания
      if !local_rx.is_empty() || local_rx.try_recv().is_ok() {
        log::info!("Main loop caught cancel signal");
        let _ = app_c.emit("cancel-download-version", &version_name_c);
      }

      let file_path = download_dir_c.join(&file_c.path);
      log::debug!("Start download file: {:?}", file_path);
      let api_client = {
        let service_guard = service_c.lock().await;
        service_guard.api_client.clone()
      };

      // Выполняем загрузку
      let download_result = tokio::select! {
          _ = local_rx.recv() => {
              // Если пришел сигнал отмены в процессе скачивания
              log::info!("Task for {} received cancel", file_c.name);
              let _ = app_c.emit("cancel-download-version", &version_name_c);
              return;
          }
          res = service_files_c.download_blob_to_file(
              &api_client,
              &version_name_c,
              &file_c.project_id,
              &file_c.id,
              &file_path,
              &None,
              local_2_rx
          ) => res
      };

      if let Err(e) = download_result {
        log::error!("Error downloading {}: {}", file_c.name, e);
        return;
      }

      // Обновляем состояние и конфиг
      let current_downloaded = downloaded_cnt_c.fetch_add(1, Ordering::SeqCst) + 1;

      {
        let mut config_guard = app_config_c.lock().await;
        if let Some(ver) = config_guard.progress_download.get_mut(&version_name_c) {
          if let Some(file_progress) = ver.files.get_mut(&file_c.id) {
            file_progress.is_downloaded = true;
          }
          ver.downloaded_files_cnt = current_downloaded;
        }
        let _ = config_guard.save();
      }

      // Эмит прогресса
      let progress = (current_downloaded as f32 / total_file_count as f32) * 100.0;
      let _ = app_c.emit(
        "download-version",
        DownloadProgress {
          version_name: version_name_c,
          status: DownloadStatus::DownloadFiles,
          file: file_c.name,
          progress,
          downloaded_files_cnt: current_downloaded,
          total_file_count,
        },
      );
    });

    join_handles.push(handle);
  }

  // Ждем завершения всех запущенных задач
  for h in join_handles {
    let _ = h.await;
  }

  let _ = app.emit("download-unpack-version", &versionName);
  Ok(())
}
