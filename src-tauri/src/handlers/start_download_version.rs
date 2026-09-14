use crate::{
  configs::AppConfig::{AppConfig, FileProgress, VersionProgress},
  handlers::dto::{DownlaodFileStat, DownloadProgress, DownloadStatus, ManifestFileKind},
  service::{files::ServiceFiles, get_release::ServiceGetRelease, main::Service, unpack::ServiceUnpacker},
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
  tokio::time::sleep(std::time::Duration::from_millis(500)).await;

  // Final defensive save of the whole config.
  let mut config_guard = app_config.lock().await;
  let _ = config_guard.save();
  Ok(())
}

/// Fill sha256/kind/target of a FileProgress from the version manifest
/// (manifest v2). Old manifests simply leave the defaults (size-only checks).
fn enrich_file_progress(file: &mut FileProgress, manifest: Option<&crate::handlers::dto::ReleaseManifest>) {
  let Some(manifest) = manifest else { return };
  let Some(entry) = manifest.files.iter().find(|f| f.name == file.name) else {
    return;
  };
  file.sha256 = entry.sha256.clone();
  file.kind = entry.kind;
  file.target = entry.target.clone();
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
    return Err(crate::consts::ERR_DOWNLOAD_ALREADY_RUNNING.to_string());
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
    return Err(crate::consts::ERR_USER_CANCELLED.to_string());
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
    let mut fp = FileProgress {
      id: file.name.clone(),
      download_link: file.download_link.clone(),
      name: file.name.clone(),
      is_downloaded: false,
      is_unpacked: false,
      size: 0,
      total_size: file.size,
      sha256: None,
      kind: ManifestFileKind::Zip,
      target: None,
      net_retries: 0,
      verify_retries: 0,
      last_error: None,
    };
    enrich_file_progress(&mut fp, version.manifest.as_ref());
    version.files.insert(file.name.clone(), fp);
  }

  if rx.try_recv().is_ok() {
    log::info!("Download task '{}' was cancelled", &versionName);
    return Err(crate::consts::ERR_USER_CANCELLED.to_string());
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

  // The download queue is the full file list; nothing to post-process upfront.
  let files_to_download: Vec<FileProgress> = version.files.values().cloned().collect();

  let api_client = {
    let service_guard = service.lock().await;
    service_guard.api_client.clone()
  };

  crate::service::download_worker::run_version_pipeline(
    &app,
    &app_config.inner().clone(),
    &service_files.inner().clone(),
    &service_unpack.inner().clone(),
    api_client,
    &version,
    files_to_download,
    Vec::new(),
    cancel_tx,
  )
  .await
}
