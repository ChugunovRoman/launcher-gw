use crate::{
  configs::AppConfig::{AppConfig, FileProgress, Version, VersionProgress},
  handlers::dto::{DownloadProgress, DownloadStatus},
  service::{files::ServiceFiles, get_release::ServiceGetRelease, main::Service},
  utils::errors::log_full_error,
};
use anyhow::Context;
use std::{
  collections::HashMap,
  path::Path,
  sync::{Arc, Mutex as StdMutex},
};
use tauri::Emitter;
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
    channel_map.lock().unwrap().insert(versionName.clone(), tx);
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

  log::info!("start_download_versions, selected_version: {:?}", selected_version);

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
      .get_main_release_files(selected_version.id)
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
    version.files.insert(
      file.id.clone(),
      FileProgress {
        id: file.id.clone(),
        project_id: file.project_id.clone(),
        name: file.name.clone(),
        path: file.path.clone(),
        is_downloaded: false,
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

  let mut downloaded_files_cnt: u16 = 0;

  for file in files {
    if rx.try_recv().is_ok() {
      log::info!("Download task '{}' was cancelled", &versionName);
      return Err("USER_CANCELLED".to_string());
    }

    log::info!("Get file {:?}", file.name);

    let mut progress = (downloaded_files_cnt as f32 / version.total_file_count as f32) * 100.0;
    let _ = app.emit(
      "download-version",
      DownloadProgress {
        version_name: version.name.clone(),
        status: DownloadStatus::DownloadFiles,
        file: file.name.clone(),
        progress,
        downloaded_files_cnt,
        total_file_count: version.total_file_count,
      },
    );

    let file_path = download_dir.join(&file.path);

    let api_client = {
      let service_guard = service.lock().await;
      service_guard.api_client.clone()
    };
    service_files
      .download_blob_to_file(&api_client, &version.name, &file.project_id, &file.id, &file_path)
      .await
      .with_context(|| format!("Failed to download release file: {:?} for version: {}", file_path, version.name))
      .map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

    log::info!("Download file complete {:?}", &file.name);

    {
      let mut config_guard = app_config.lock().await;

      if let Some(ver) = config_guard.progress_download.get_mut(&version.name) {
        if let Some(file_progress) = ver.files.get_mut(&file.id) {
          file_progress.is_downloaded = true;
        }
        ver.downloaded_files_cnt += 1;
      }
      config_guard.save().map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;
    }

    downloaded_files_cnt += 1;
    progress = (downloaded_files_cnt as f32 / version.total_file_count as f32) * 100.0;

    log::info!(
      "download_files_progress: {} ({}/{})",
      progress,
      downloaded_files_cnt,
      version.total_file_count,
    );

    let _ = app.emit(
      "download-version",
      DownloadProgress {
        version_name: version.name.clone(),
        status: DownloadStatus::DownloadFiles,
        file: file.name,
        progress,
        downloaded_files_cnt,
        total_file_count: version.total_file_count,
      },
    );
  }

  if rx.try_recv().is_ok() {
    log::info!("Download task '{}' was cancelled", &versionName);
    return Err("USER_CANCELLED".to_string());
  }

  let _ = app.emit("download-unpack-version", &version.name);

  Ok(())
}
