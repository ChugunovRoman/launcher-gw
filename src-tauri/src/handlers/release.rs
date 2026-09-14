use crate::{
  configs::AppConfig::{AppConfig, Version},
  consts::BIN_DIR,
  handlers::dto::{DownlaodFileStat, ReleaseManifest},
  providers::dto::AssetSha256,
  service::{create_release::ServiceRelease, get_release::{ServiceGetRelease, ReleaseSource}, main::Service},
  utils::{errors::{log_full_error, upload_log}, git::grouping::group_files_by_size, patch_markers::{read_installed_patches, write_patch_marker}, resources::game_exe},
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

  // load_manifest uses the GitHub Search API (anonymous rate limit ~10/min).
  // For anonymous GitHub players the static release index already provides
  // everything get_releases needs, so skip it — same guard as tauri_setup.
  // GitLab uses a hardcoded manifest (no network), token-holders have quota.
  let should_skip = {
    match service_guard.api_client.current_provider() {
      Ok(api) => !api.is_suppot_subgroups() && api.get_token().is_empty(),
      Err(_) => false,
    }
  };
  if !should_skip {
    service_guard.load_manifest().await.map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;
  } else {
    log::info!("get_available_versions: skipping load_manifest (GitHub player mode, no token)");
  }

  let releases = service_guard.get_releases(ReleaseSource::Cached).await.context("Cannot get game releases").map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  // C7: record which provider these versions belong to.
  let provider_id = service_guard.api_client.current_provider()
    .ok().map(|api| api.id().to_string());

  {
    let mut config_guard = app_config.lock().await;
    config_guard.versions = releases.clone();
    config_guard.versions_provider_id = provider_id;
    config_guard.save().map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;
  }

  Ok(releases)
}

/// Force-refresh the releases list (invalidates cache, re-fetches index).
/// Dev-only: also merges API-only releases when a token is present.
#[tauri::command]
pub async fn refresh_available_versions(app: tauri::AppHandle, app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>) -> Result<Vec<Version>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let mut service_guard = state.lock().await;

  let releases = service_guard.refresh_releases().await.context("Cannot refresh releases").map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  let provider_id = service_guard.api_client.current_provider()
    .ok().map(|api| api.id().to_string());

  {
    let mut config_guard = app_config.lock().await;
    config_guard.versions = releases.clone();
    config_guard.versions_provider_id = provider_id;
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
  let mut service_guard = state.lock().await;

  let api = service_guard.api_client.current_provider().map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  let manifest = api.get_manifest().map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;
  let parent_id = manifest
    .root_id
    .ok_or_else(|| format!("root_id is not set for {} provider", api.id()))?;
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

  // Invalidate in-memory cache so the new release appears on next fetch.
  service_guard.invalidate_releases();

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
pub async fn get_local_version(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>) -> Result<Vec<Version>, String> {
  let cfg = app_config.lock().await;
  crate::service::get_release::get_local_version_from_config(&cfg)
    .await
    .map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })
}

#[tauri::command]
pub async fn get_main_version(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>) -> Result<Option<Version>, String> {
  let cfg = app_config.lock().await;
  Ok(crate::service::get_release::get_main_version_from_config(&cfg).await)
}

#[tauri::command]
pub async fn get_installed_versions(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>) -> Result<Vec<Version>, String> {
  // Collect versions that need migration (config installed_updates -> marker files).
  let mut migration_keys: Vec<String> = Vec::new();

  let versions = {
    let cfg = app_config.lock().await;
    let mut result: Vec<Version> = Vec::new();

    for (key, v) in cfg.installed_versions.iter() {
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
      if !path.exists() || !((path_bin.exists() && path_exe.exists()) || (engine_path_exists && fsgame_path_exists)) || !path.is_dir() {
        continue;
      }

      // One-time migration: if config still has installed_updates, write them
      // as marker files (defer config clearing to avoid borrow conflict).
      if !v.installed_updates.is_empty() {
        log::info!("Migrating {} installed_updates from config to markers for '{}'", v.installed_updates.len(), &v.name);
        for patch in &v.installed_updates {
          if let Err(e) = write_patch_marker(path, patch) {
            log::warn!("Migration: cannot write marker for '{}': {}", patch.name, e);
          }
        }
        migration_keys.push(key.clone());
      }

      let mut ver = v.clone();
      // Always read from marker files (source of truth).
      ver.installed_updates = read_installed_patches(path);
      result.push(ver);
    }

    result
  };

  // Apply migration: clear config fields and save.
  if !migration_keys.is_empty() {
    let mut cfg = app_config.lock().await;
    for key in &migration_keys {
      if let Some(version) = cfg.installed_versions.get_mut(key) {
        version.installed_updates.clear();
      }
    }
    if let Err(e) = cfg.save() {
      log::warn!("get_installed_versions: config save after migration failed: {}", e);
    }
  }

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
      .cloned()
      .ok_or_else(|| format!("add_installed_version_from_config() version not found: {} !", &versionName))?
  };

  {
    let mut config_guard = app_config.lock().await;

    // `start_repair_version` finishes through this very same
    // download-unpack-version → add_installed_version_from_config path as a
    // fresh install (both end in the shared `finalize_download`). When an
    // entry for this version already exists, MERGE instead of overwriting:
    // a repair only re-downloads a handful of broken files, it must not wipe
    // engine_path/fsgame_path/userltx_path (set via RunParams) or the
    // installed-patch list of an already-installed version (bug fix).
    let existing = config_guard.installed_versions.get(&version.path).cloned();

    config_guard.installed_versions.insert(
      version.path.clone(),
      Version {
        id: version.id,
        name: version.name.clone(),
        path: version.path.clone(),
        manifest: version.manifest.clone(),
        installed_path: version.installed_path.clone(),
        download_path: version.download_path.clone(),
        engine_path: existing.as_ref().and_then(|v| v.engine_path.clone()),
        fsgame_path: existing.as_ref().and_then(|v| v.fsgame_path.clone()),
        userltx_path: existing.as_ref().and_then(|v| v.userltx_path.clone()),
        exe_path: version
          .manifest
          .as_ref()
          .and_then(|m| m.exe_path.clone())
          .or_else(|| existing.as_ref().and_then(|v| v.exe_path.clone())),
        installed_updates: existing.map(|v| v.installed_updates).unwrap_or_default(),
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
    exe_path: None,
    installed_updates: vec![],
    is_local: true,
  };

  {
    let mut config_guard = app_config.lock().await;

    let version_name = version.name.clone();
    config_guard.installed_versions.insert(version.path.clone(), version);

    if let Some(ver) = config_guard.progress_download.get_mut(&version_name) {
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
    exe_path: None,
    installed_updates: vec![],
    is_local: true,
  };

  {
    let mut config_guard = app_config.lock().await;

    let version_name = version.name.clone();
    config_guard.installed_versions.insert(version.path.clone(), version);

    if let Some(ver) = config_guard.progress_download.get_mut(&version_name) {
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
        let size = if file_part_path.exists() {
          match tokio::fs::read_to_string(&file_part_path).await {
            Ok(content) => content.trim().parse::<u64>().unwrap_or(0),
            Err(_) => 0,
          }
        } else {
          // No .part → treat as finished download; prefer on-disk size.
          match tokio::fs::metadata(&file_path).await {
            Ok(meta) => meta.len(),
            Err(_) => file.1.total_size,
          }
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

// ---------------------------------------------------------------------------
// "Get SHA" developer tool: server-side hashes of a published release's
// assets, so a manifest can be filled in without re-packing (see stage 6 of
// the download-integrity plan).
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_release_assets_sha(
  app: tauri::AppHandle,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  name: String,
  patch_tag: Option<String>,
) -> Result<ReleaseManifest, String> {
  let api_client = {
    let svc = service.lock().await;
    svc.api_client.clone()
  };
  let api = api_client.current_provider().map_err(|e| e.to_string())?;
  let provider_id = api.id().to_string();

  // 1. The index is the source of truth for tag + the manifest's own URL.
  let index = crate::service::index::load_index(&provider_id)
    .await
    .map_err(|e| format!("Cannot load release index: {}", e))?;
  let entry = index
    .releases
    .iter()
    .find(|r| r.name == name || r.path == name)
    .ok_or_else(|| crate::consts::ERR_RELEASE_NOT_IN_INDEX.to_string())?;

  // 2. Resolve the repo that owns the assets AND fetch the manifest fresh
  // (not the possibly-stale index copy) — its file list is what gets the
  // sha256 fields filled in below.
  let (project_id, tag, mut manifest) = if let Some(patch_tag) = patch_tag.as_deref().filter(|t| !t.is_empty()) {
    let patch = entry
      .patches
      .iter()
      .find(|p| p.tag == patch_tag)
      .ok_or_else(|| format!("Patch '{}' not found in index of release '{}'", patch_tag, &name))?;
    let manifest_url = patch
      .manifest
      .as_deref()
      .ok_or_else(|| format!("Patch '{}' has no manifest URL in the index (re-run publish_index)", patch_tag))?;
    let manifest = crate::handlers::patch_install::download_manifest(&api_client, manifest_url)
      .await
      .map_err(|e| format!("Cannot fetch patch manifest: {}", e))?;
    let project = crate::handlers::patch_install::resolve_updates_project(&api_client, &entry.name)
      .await
      .map_err(|e| e.to_string())?;
    let pid = crate::handlers::patch_install::project_id_for(&api_client, &project).map_err(|e| e.to_string())?;
    (pid, patch_tag.to_string(), manifest)
  } else {
    let repos = api.get_release_repos_by_name(&entry.name).await.map_err(|e| e.to_string())?;
    let main = repos
      .iter()
      .find(|r| crate::service::get_release::is_main_repo(&r.name))
      .or_else(|| repos.first())
      .ok_or_else(|| format!("No repositories found for release '{}'", &entry.name))?;
    let pid = if api.is_suppot_subgroups() { main.id.to_string() } else { main.name.clone() };
    let manifest = {
      let service_guard = service.lock().await;
      service_guard
        .get_release_manifest(&entry.name)
        .await
        .map_err(|e| format!("Cannot fetch release manifest: {}", e))?
    };
    (pid, entry.tag.clone(), manifest)
  };

  // 3. Ask the provider for server-side hashes, then fill them into the
  // manifest's own file list (skipping the `manifest.json` asset entry
  // itself, which never gets a data hash).
  let remote = api.get_release_assets_sha256(&project_id, &tag).await.map_err(|e| e.to_string())?;
  let mut by_name: std::collections::HashMap<String, AssetSha256> = remote.into_iter().map(|a| (a.name.clone(), a)).collect();

  for file in manifest.files.iter_mut() {
    if file.kind == crate::handlers::dto::ManifestFileKind::Manifest {
      continue;
    }
    match by_name.remove(&file.name) {
      Some(asset) => {
        if asset.sha256.is_none() {
          upload_log(&app, format!("File '{}' has no server-side sha256 (uploaded before the provider added hash reporting)", &file.name));
        } else {
          file.sha256 = asset.sha256;
        }
      }
      None => {
        upload_log(&app, format!("Warning: file '{}' (listed in the manifest) not found on server (tag '{}')", &file.name, &tag));
      }
    }
  }
  for (_, extra) in by_name {
    upload_log(&app, format!("Warning: server has extra file '{}' not present in the manifest", extra.name));
  }

  // Bump the schema so the download side starts verifying hashes for this
  // release/patch once the developer pastes this manifest back.
  if manifest.schema < 2 {
    manifest.schema = 2;
  }

  // 4. The full, updated manifest.json — the developer pastes this over the
  // existing file (repo commit for a release, re-upload for a patch asset).
  let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
  upload_log(&app, format!("Updated manifest.json for '{}' (tag '{}') — paste this over the existing file:", &name, &tag));
  upload_log(&app, json.clone());
  log::info!("get_release_assets_sha '{}': {}", &name, &json);

  Ok(manifest)
}
