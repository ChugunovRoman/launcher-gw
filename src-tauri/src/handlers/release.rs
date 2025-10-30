use crate::{
  configs::AppConfig::{AppConfig, FileProgress, Version, VersionProgress},
  consts::{MANIFEST_NAME, VERSIONS_DIR},
  handlers::dto::DownloadProgress,
  providers::dto::TreeItem,
  service::{files::Servicefiles, get_release::ServiceRelease, main::Service},
};
use anyhow::{Context, Result as AnyhowResult}; // переименовываем, чтобы не конфликтовало
use std::{
  collections::HashMap,
  path::Path,
  sync::{Arc, Mutex},
};
use tauri::{Emitter, Manager};

// Вспомогательная функция с anyhow
async fn get_available_versions_inner(app: &tauri::AppHandle, app_config: &Arc<Mutex<AppConfig>>) -> AnyhowResult<Vec<Version>> {
  let service = app.state::<Service>();
  let releases = service.get_releases().await.context("Cannot get game releases")?;

  {
    let mut config_guard = app_config.lock().map_err(|_| anyhow::anyhow!("Failed to lock app config"))?;
    config_guard.versions = releases.clone();
  }

  Ok(releases)
}

#[tauri::command]
pub async fn get_available_versions(app: tauri::AppHandle, app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>) -> Result<Vec<Version>, String> {
  get_available_versions_inner(&app, &app_config).await.map_err(|e| e.to_string())
}

async fn start_download_versions_inner(app: &tauri::AppHandle, app_config: &Arc<Mutex<AppConfig>>, version_id: u32) -> AnyhowResult<()> {
  let service = app.state::<Service>();

  let cfg = app_config.lock().map_err(|_| anyhow::anyhow!("Lock failed"))?.clone();

  let selected_version = cfg
    .versions
    .iter()
    .find(|v| v.id == version_id)
    .ok_or_else(|| anyhow::anyhow!("Version with id {} not found", version_id))?;

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
  std::fs::create_dir_all(&download_dir).with_context(|| format!("Failed to create output download directory: {:?}", download_dir))?;

  let all_files = service
    .get_main_release_files(version_id)
    .await
    .context("Failed to get main release files")?;

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
    let data = service
      .fetch_manifest_from_blob(&manifest.project_id, &manifest.id)
      .await
      .context("Failed to fetch manifest from blob")?;
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
    let mut config_guard = app_config.lock().map_err(|_| anyhow::anyhow!("Failed to lock app config"))?;
    config_guard.progress.insert(version.id, version.clone());
    config_guard.save();
  }

  let _ = app.emit("download_finish_get_file_list", 0);

  let mut downloaded_files_cnt: u16 = 0;

  for file in files {
    log::info!("Get file {:?}", file.name);

    let file_path = download_dir.join(&file.path);

    service
      .download_blob_to_file(&file.project_id, &file.id, &file_path)
      .await
      .with_context(|| format!("Failed to download release file: {:?} for version: {}", file_path, version.name))?;

    log::info!("Download file complete {:?}", file.name);

    {
      let mut config_guard = app_config.lock().map_err(|_| anyhow::anyhow!("Failed to lock app config"))?;

      if let Some(ver) = config_guard.progress.get_mut(&version.id) {
        if let Some(file_progress) = ver.files.get_mut(&file.id) {
          file_progress.is_downloaded = true;
        }
      }
      config_guard.save();
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
    let mut config_guard = app_config.lock().map_err(|_| anyhow::anyhow!("Failed to lock app config"))?;

    config_guard.installed_versions.insert(
      version.id,
      Version {
        id: version.id,
        name: version.name,
        path: version.path,
        installed_updates: vec![],
      },
    );

    if let Some(ver) = config_guard.progress.get_mut(&version.id) {
      ver.is_downloaded = true;
    }

    config_guard.save();
  }

  Ok(())
}

#[tauri::command]
pub async fn start_download_version(
  app: tauri::AppHandle,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  version_id: u32,
) -> Result<(), String> {
  start_download_versions_inner(&app, &app_config, version_id)
    .await
    .map_err(|e| e.to_string())
}
