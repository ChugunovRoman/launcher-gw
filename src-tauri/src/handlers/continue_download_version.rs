use crate::{
  configs::AppConfig::{AppConfig, FileProgress, Version},
  handlers::{
    dto::{DownloadProgress, DownloadStatus},
    start_download_version::CancelMap,
  },
  service::{files::ServiceFiles, main::Service},
  utils::errors::log_full_error,
};
use anyhow::Context;
use std::{collections::HashMap, path::Path, sync::Arc};
use tauri::Emitter;
use tokio::sync::{Mutex, broadcast};

#[tauri::command]
pub async fn continue_download_version(
  app: tauri::AppHandle,
  channel_map: tauri::State<'_, CancelMap>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  service_files: tauri::State<'_, Arc<ServiceFiles>>,
  versionName: String,
) -> Result<(), String> {
  log::info!("Start continue_download_version, selected_version: {:?}", &versionName);

  let (tx, mut rx) = broadcast::channel::<()>(1);
  {
    channel_map.lock().unwrap().insert(versionName.clone(), tx);
  }
  // Удаляем запись после завершения (успешного или нет)
  scopeguard::defer! {
    channel_map.lock().unwrap().remove(&versionName);
  };

  let cfg = app_config.lock().await.clone();

  let version = cfg
    .progress_download
    .iter()
    .find(|v| v.0 == &versionName)
    .ok_or_else(|| anyhow::anyhow!("Version not found in progress_download by name: {:?}", &versionName))
    .map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

  let progress = (version.1.downloaded_files_cnt as f32 / version.1.total_file_count as f32) * 100.0;

  let _ = app.emit(
    "download-version",
    DownloadProgress {
      version_name: version.1.name.clone(),
      status: DownloadStatus::Init,
      file: "".to_owned(),
      progress,
      downloaded_files_cnt: version.1.downloaded_files_cnt,
      total_file_count: version.1.total_file_count,
    },
  );

  let download_dir = Path::new(&version.1.download_path);
  std::fs::create_dir_all(&download_dir)
    .with_context(|| format!("Failed to create output download directory: {:?}", download_dir))
    .map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

  let files: HashMap<String, FileProgress> = version
    .1
    .files
    .iter()
    .filter(|(_, f)| !f.is_downloaded)
    .map(|(k, v)| (k.clone(), v.clone()))
    .collect();

  let mut downloaded_files_cnt: u16 = version.1.total_file_count - files.len() as u16;

  let _ = app.emit(
    "download-version",
    DownloadProgress {
      version_name: version.1.name.clone(),
      status: DownloadStatus::Init,
      file: "".to_owned(),
      progress: 0.0,
      downloaded_files_cnt,
      total_file_count: version.1.total_file_count,
    },
  );

  for (file_name, file) in files {
    log::info!("Get file {:?}", file_name);

    if rx.try_recv().is_ok() {
      log::info!("Download task '{}' was cancelled", &versionName);
      return Err("USER_CANCELLED".to_string());
    }

    let mut progress = (downloaded_files_cnt as f32 / version.1.total_file_count as f32) * 100.0;
    let _ = app.emit(
      "download-version",
      DownloadProgress {
        version_name: version.1.name.clone(),
        status: DownloadStatus::DownloadFiles,
        file: file.name.clone(),
        progress,
        downloaded_files_cnt,
        total_file_count: version.1.total_file_count,
      },
    );

    let file_path = download_dir.join(&file.path);

    let api_client = {
      let service_guard = service.lock().await;
      service_guard.api_client.clone()
    };
    service_files
      .download_blob_to_file(&api_client, &version.1.name, &file.project_id, &file.id, &file_path)
      .await
      .with_context(|| format!("Failed to download release file: {:?} for version: {}", file_path, version.1.name))
      .map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

    log::info!("Download file complete {:?}", file_name);

    {
      let mut config_guard = app_config.lock().await;

      if let Some(ver) = config_guard.progress_download.get_mut(&version.1.name) {
        if let Some(file_progress) = ver.files.get_mut(&file.id) {
          file_progress.is_downloaded = true;
        }
      }
      config_guard.save().map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;
    }

    downloaded_files_cnt += 1;
    progress = (downloaded_files_cnt as f32 / version.1.total_file_count as f32) * 100.0;

    log::info!(
      "download_files_progress: {} ({}/{})",
      progress,
      downloaded_files_cnt,
      version.1.total_file_count,
    );

    let _ = app.emit(
      "download-version",
      DownloadProgress {
        version_name: version.1.name.clone(),
        status: DownloadStatus::DownloadFiles,
        file: file.name,
        progress,
        downloaded_files_cnt,
        total_file_count: version.1.total_file_count,
      },
    );
  }

  let _ = app.emit("download-unpack-version", &version.1.name);

  Ok(())
}
