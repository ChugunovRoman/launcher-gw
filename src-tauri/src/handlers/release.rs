use crate::{
  configs::AppConfig::{AppConfig, FileProgress, Version, VersionProgress},
  consts::{MANIFEST_NAME, VERSIONS_DIR},
  handlers::dto::DownloadProgress,
  providers::dto::TreeItem,
  service::{create_release::ServiceRelease, files::Servicefiles, get_release::ServiceGetRelease, main::Service},
  utils::{errors::log_full_error, git::grouping::group_files_by_size},
};
use anyhow::Context;
use std::convert::TryFrom;
use std::{collections::HashMap, path::Path, sync::Arc};
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

#[tauri::command]
pub async fn get_available_versions(app: tauri::AppHandle, app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>) -> Result<Vec<Version>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;
  let releases = service_guard.get_releases().await.context("Cannot get game releases").map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  {
    let mut config_guard = app_config.lock().await;
    config_guard.versions = releases.clone();
  }

  Ok(releases)
}

#[tauri::command]
pub async fn start_download_version(
  app: tauri::AppHandle,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  version_id: u32,
) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;

  let cfg = app_config.lock().await.clone();

  let selected_version = cfg
    .versions
    .iter()
    .find(|v| v.id == version_id)
    .ok_or_else(|| anyhow::anyhow!("Version with id {} not found", version_id))
    .map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

  log::info!("start_download_versions, selected_version: {:?}", selected_version);

  let mut version = VersionProgress {
    id: selected_version.id,
    name: selected_version.name.clone(),
    path: selected_version.path.clone(),
    is_downloaded: false,
    files: HashMap::new(),
    file_count: 0,
    manifest: None,
  };

  let _ = app.emit("download_start_get_file_list", 0);

  let download_dir = Path::new(&cfg.install_path).join(VERSIONS_DIR).join(format!("{}_data", version.path));
  std::fs::create_dir_all(&download_dir)
    .with_context(|| format!("Failed to create output download directory: {:?}", download_dir))
    .map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

  let all_files = service_guard
    .get_main_release_files(version_id)
    .await
    .context("Failed to get main release files")
    .map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

  let mut files = Vec::new();
  let mut manifest_blob: Option<TreeItem> = None;

  for file in all_files {
    if file.item_type != "blob" {
      continue;
    }

    if file.name == MANIFEST_NAME {
      manifest_blob = Some(file);
    } else {
      files.push(file);
    }
  }

  // TODO: подумать как потом прокинуть манифет на фронт
  if let Some(manifest) = manifest_blob {
    let data = service_guard
      .fetch_manifest_from_blob(&manifest.project_id, &manifest.id)
      .await
      .context("Failed to fetch manifest from blob")
      .map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;
    version.manifest = Some(data);
  }

  version.file_count = files.len() as u16;

  for file in &files {
    version.files.insert(
      file.id.clone(),
      FileProgress {
        id: file.id.clone(),
        name: file.name.clone(),
        path: file.path.clone(),
        is_downloaded: false,
      },
    );
  }

  {
    let mut config_guard = app_config.lock().await;
    config_guard.progress_download.insert(version.id, version.clone());
    config_guard.save().map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;
  }

  let _ = app.emit("download_finish_get_file_list", 0);

  let mut downloaded_files_cnt: u16 = 0;

  for file in files {
    log::info!("Get file {:?}", file.name);

    let file_path = download_dir.join(&file.path);

    service_guard
      .download_blob_to_file(&file.project_id, &file.id, &file_path)
      .await
      .with_context(|| format!("Failed to download release file: {:?} for version: {}", file_path, version.name))
      .map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

    log::info!("Download file complete {:?}", file.name);

    {
      let mut config_guard = app_config.lock().await;

      if let Some(ver) = config_guard.progress_download.get_mut(&version.id) {
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
    let progress = (downloaded_files_cnt as f32 / version.file_count as f32) * 100.0;

    log::info!("download_files_progress: {} ({}/{})", progress, downloaded_files_cnt, version.file_count,);

    let _ = app.emit(
      "download_files_progress",
      DownloadProgress {
        progress,
        downloaded_files_cnt,
        total_file_count: version.file_count,
      },
    );
  }

  {
    let mut config_guard = app_config.lock().await;

    config_guard.installed_versions.insert(
      version.path.clone(),
      Version {
        id: version.id,
        name: version.name,
        path: version.path,
        installed_updates: vec![],
        is_local: false,
      },
    );

    if let Some(ver) = config_guard.progress_download.get_mut(&version.id) {
      ver.is_downloaded = true;
    }

    config_guard.save().map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;
  }

  Ok(())
}

#[tauri::command]
pub async fn create_release_repos(app: tauri::AppHandle, name: String, path: String) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;

  let api = service_guard.api_client.current_provider().map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  let manifest = api.get_manifest().map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;
  let parent_id_str = manifest.root_id.expect(&format!("root_id is not set for {} provider", api.id()));
  let parent_id: u32 = parent_id_str
    .parse()
    .expect(&format!("Cannot conver root_id to u32, parent_id_str: {}", &parent_id_str));

  let base_dir = Path::new(&path);
  let groups = group_files_by_size(base_dir, manifest.max_size).map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;
  let cnt: u16 = u16::try_from(groups.len()).expect("create_release_repos|groups.len() Value too large for u16");

  let main_cnt: u16 = cnt;
  let updates_cnt: u16 = 2;

  let _ = service_guard
    .create_release_repos(&name, &parent_id, &main_cnt, &updates_cnt)
    .await
    .map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

  Ok(())
}

#[tauri::command]
pub async fn get_local_version(app: tauri::AppHandle) -> Result<Vec<Version>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;
  let versions = service_guard.get_local_version().await.map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  Ok(versions)
}
