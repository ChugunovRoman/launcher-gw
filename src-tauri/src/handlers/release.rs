use crate::{
  configs::AppConfig::{AppConfig, Version},
  consts::BIN_DIR,
  handlers::dto::{DownlaodFileStat, ReleaseManifest},
  service::{create_release::ServiceRelease, get_release::ServiceGetRelease, main::Service},
  utils::{errors::log_full_error, git::grouping::group_files_by_size, resources::game_exe},
};
use anyhow::Context;
use std::{cmp::Reverse, fs, path::PathBuf};
use std::{path::Path, sync::Arc};
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

#[tauri::command]
pub async fn get_available_versions(app: tauri::AppHandle, app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>) -> Result<Vec<Version>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let mut service_guard = state.lock().await;
  service_guard.load_manifest().await.map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;
  let releases = service_guard.get_releases(true).await.context("Cannot get game releases").map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  {
    let mut config_guard = app_config.lock().await;
    config_guard.versions = releases.clone();
    config_guard.save().map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;
  }

  Ok(releases)
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
  let parent_id = manifest.root_id.expect(&format!("root_id is not set for {} provider", api.id()));
  let base_dir = Path::new(&path);
  let groups = group_files_by_size(base_dir, manifest.max_size).map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;
  // let cnt: u16 = u16::try_from(groups.len()).expect("create_release_repos|groups.len() Value too large for u16");

  let main_cnt: u16 = 1;
  let updates_cnt: u16 = 1;

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
pub async fn get_release_manifest(app: tauri::AppHandle, releaseName: String) -> Result<ReleaseManifest, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;
  let release = {
    let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("AppConfig not initialized")?;
    let config_guard = state.lock().await;
    config_guard
      .versions
      .iter()
      .find(|r| r.name == releaseName)
      .ok_or_else(|| "Release not found".to_string())?
      .clone()
  };
  let manifest = {
    log::info!("manifest in config for release: {:?}", &release);

    match release.manifest.clone() {
      Some(data) => data,
      None => {
        let file = service_guard.get_release_manifest(&release.name).await.map_err(|e| {
          log_full_error(&e);
          e.to_string()
        })?;
        log::info!("load manifest from Gitlab");

        {
          let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("AppConfig not initialized")?;
          let mut config_guard = state.lock().await;
          let version = config_guard
            .versions
            .iter_mut()
            .find(|r| r.name == releaseName)
            .ok_or_else(|| "Release not found".to_string())?;

          version.manifest = Some(file.clone());

          config_guard.save().map_err(|e| {
            log_full_error(&e);
            e.to_string()
          })?;
        }

        file
      }
    }
  };

  Ok(manifest)
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

#[tauri::command]
pub async fn get_main_version(app: tauri::AppHandle) -> Result<Option<Version>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;

  Ok(service_guard.get_main_version().await)
}

#[tauri::command]
pub async fn get_installed_versions(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>) -> Result<Vec<Version>, String> {
  let versions = {
    let cfg = app_config.lock().await;

    cfg
      .installed_versions
      .iter()
      .filter_map(|(_, v)| {
        let path = Path::new(&v.installed_path);
        let engine_path_exists = match &v.engine_path {
          Some(value) => Path::new(value).exists(),
          None => false,
        };
        let fsgame_path_exists = match &v.fsgame_path {
          Some(value) => Path::new(value).exists(),
          None => false,
        };
        let path_bin = path.join(BIN_DIR);
        let path_exe = path_bin.join(game_exe());
        log::debug!(
          "get_installed_versions, filter version: {} installed_path: {} game_exe: {}",
          &v.name,
          &v.installed_path,
          game_exe()
        );
        if path.exists() && ((path_bin.exists() && path_exe.exists()) || (engine_path_exists && fsgame_path_exists)) && path.is_dir() {
          Some(v.clone())
        } else {
          None
        }
      })
      .collect()
  };

  Ok(versions)
}

#[tauri::command]
pub async fn delete_installed_version(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>, versionName: String) -> Result<(), String> {
  let version = {
    let cfg = app_config.lock().await;
    cfg.installed_versions.get(&versionName).cloned()
  };

  if let Some(v) = version {
    fs::remove_dir_all(Path::new(&v.installed_path)).map_err(|e| e.to_string())?;

    {
      let mut config_guard = app_config.lock().await;

      let _ = config_guard.installed_versions.remove(&versionName);

      config_guard.save().map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;
    }
  }

  Ok(())
}

#[tauri::command]
pub async fn has_root_version() -> Result<bool, String> {
  let curr_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

  let bin = curr_dir.join(BIN_DIR);
  if !bin.exists() {
    return Ok(false);
  }

  let xr_engine = bin.join(game_exe());
  if !xr_engine.exists() {
    return Ok(false);
  }

  Ok(true)
}

#[tauri::command]
pub async fn add_installed_version_from_config(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>, versionName: String) -> Result<(), String> {
  let version = {
    let cfg = app_config.lock().await;
    cfg
      .progress_download
      .get(&versionName)
      .expect(&format!("add_installed_version_from_config() version not found: {} !", &versionName))
      .clone()
  };

  {
    let mut config_guard = app_config.lock().await;

    config_guard.installed_versions.insert(
      version.path.clone(),
      Version {
        id: version.id,
        name: version.name.clone(),
        path: version.path.clone(),
        manifest: version.manifest.clone(),
        installed_path: version.installed_path.clone(),
        download_path: version.download_path.clone(),
        engine_path: None,
        fsgame_path: None,
        userltx_path: None,
        installed_updates: vec![],
        is_local: false,
      },
    );

    if let Some(ver) = config_guard.progress_download.get_mut(&version.name) {
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
pub async fn add_installed_version_from_local_path(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>, path: String) -> Result<(), String> {
  let p = Path::new(&path);
  let base_name = match p.file_name() {
    Some(name) => name.to_string_lossy().to_string(),
    None => path.clone(),
  };

  let version = Version {
    id: 0,
    name: base_name.clone(),
    path: base_name,
    manifest: None,
    installed_path: path.clone(),
    download_path: path.clone(),
    engine_path: None,
    fsgame_path: None,
    userltx_path: None,
    installed_updates: vec![],
    is_local: true,
  };

  {
    let mut config_guard = app_config.lock().await;

    config_guard.installed_versions.insert(
      version.path.clone(),
      Version {
        id: version.id,
        name: version.name.clone(),
        path: version.path.clone(),
        manifest: version.manifest.clone(),
        installed_path: version.installed_path.clone(),
        download_path: version.download_path.clone(),
        engine_path: version.engine_path.clone(),
        fsgame_path: version.fsgame_path.clone(),
        userltx_path: version.userltx_path.clone(),
        installed_updates: vec![],
        is_local: false,
      },
    );

    if let Some(ver) = config_guard.progress_download.get_mut(&version.name) {
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
pub async fn add_installed_version_from_ui(
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  name: String,
  path: String,
  enginePath: String,
  fsgamePath: String,
  userltxPath: String,
) -> Result<(), String> {
  let p = Path::new(&path);
  let base_name = match p.file_name() {
    Some(name) => name.to_string_lossy().to_string(),
    None => path.clone(),
  };

  let version = Version {
    id: 0,
    name: name.clone(),
    path: base_name,
    manifest: None,
    installed_path: path.clone(),
    download_path: path.clone(),
    engine_path: Some(enginePath),
    fsgame_path: Some(fsgamePath),
    userltx_path: Some(userltxPath),
    installed_updates: vec![],
    is_local: true,
  };

  {
    let mut config_guard = app_config.lock().await;

    config_guard.installed_versions.insert(
      version.path.clone(),
      Version {
        id: version.id,
        name: version.name.clone(),
        path: version.path.clone(),
        manifest: version.manifest.clone(),
        installed_path: version.installed_path.clone(),
        download_path: version.download_path.clone(),
        engine_path: version.engine_path.clone(),
        fsgame_path: version.fsgame_path.clone(),
        userltx_path: version.userltx_path.clone(),
        installed_updates: vec![],
        is_local: false,
      },
    );

    if let Some(ver) = config_guard.progress_download.get_mut(&version.name) {
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
pub async fn clear_progress_version(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>, versionName: String) -> Result<(), String> {
  {
    let mut config_guard = app_config.lock().await;

    let _ = config_guard.progress_download.remove(&versionName);

    config_guard.save().map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;
  }

  Ok(())
}

#[tauri::command]
pub async fn emit_file_list_stats(
  app: tauri::AppHandle,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  versionName: String,
) -> Result<(), String> {
  let mut file_sizes: Vec<DownlaodFileStat> = vec![];

  if let Some(version) = {
    let config_guard = app_config.lock().await;

    config_guard.progress_download.get(&versionName).cloned()
  } {
    for file in version.files.iter() {
      let file_path = Path::new(&version.download_path).join(&file.1.name);
      let file_part_path = Path::new(&version.download_path).join(format!("{}.part", &file.1.name));

      if file_path.exists() {
        let size = match tokio::fs::read_to_string(file_part_path).await {
          Ok(content) => content.trim().parse::<u64>().unwrap_or(0),
          Err(_) => file.1.total_size,
        };

        file_sizes.push(DownlaodFileStat {
          name: file.1.name.clone(),
          unpacked: file.1.is_unpacked,
          size: Some(size),
        });
      } else if file.1.is_unpacked {
        file_sizes.push(DownlaodFileStat {
          name: file.1.name.clone(),
          unpacked: file.1.is_unpacked,
          size: Some(file.1.total_size),
        });
      } else {
        file_sizes.push(DownlaodFileStat {
          name: file.1.name.clone(),
          unpacked: file.1.is_unpacked,
          size: Some(0),
        });
      }
    }
  };

  file_sizes.sort_by_key(|file| Reverse(file.size));

  let _ = app.emit("download-version-files", (&versionName, file_sizes));

  Ok(())
}
