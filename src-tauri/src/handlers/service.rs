use fs_extra::dir::{CopyOptions, TransitProcess, TransitProcessResult, move_dir_with_progress};
use std::{fs, path::Path, sync::Arc};
use tauri::Emitter;
use tauri::Manager;
use tokio::sync::Mutex;

use crate::{
  configs::AppConfig::AppConfig,
  handlers::dto::ProgressPayload,
  providers::dto::ProviderStatus,
  service::{files::ServiceFiles, main::Service},
  utils::encoding::*,
};

#[tauri::command]
pub async fn ping_all_providers(app: tauri::AppHandle) -> Result<Vec<(String, ProviderStatus)>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;

  // Clone api_client under a short lock — network I/O happens outside the mutex.
  let api_client = {
    let service_guard = state.lock().await;
    service_guard.api_client.clone()
  };

  let results: Vec<(String, ProviderStatus)> = api_client
    .ping_all()
    .await
    .into_iter()
    .map(|(id, status)| (id.to_string(), status))
    .collect();

  // Update service.stats so get_api_providers_stats returns fresh data.
  // Provider statuses are already updated inside ping() above;
  // we just rebuild the stats vec from live provider objects.
  {
    let mut service_guard = state.lock().await;
    let fresh: Vec<_> = service_guard
      .api_client
      .get_provider_ids()
      .iter()
      .filter_map(|id| {
        service_guard
          .api_client
          .get_provider(id)
          .ok()
          .map(|p| (p.id(), p.status()))
      })
      .collect();
    service_guard.stats = fresh;
  }

  Ok(results)
}
#[tauri::command]
pub async fn ping_current_provider(app: tauri::AppHandle) -> Result<(String, ProviderStatus), String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;

  let (api_client, provider_id) = {
    let service_guard = state.lock().await;
    let api = service_guard.api_client.current_provider().map_err(|e| e.to_string())?;
    (service_guard.api_client.clone(), api.id().to_owned())
  };

  let api = api_client.get_provider(&provider_id).map_err(|e| e.to_string())?;
  let status = api.ping().await;

  // Update service.stats from live provider statuses.
  {
    let mut service_guard = state.lock().await;
    let fresh: Vec<_> = service_guard
      .api_client
      .get_provider_ids()
      .iter()
      .filter_map(|id| {
        service_guard
          .api_client
          .get_provider(id)
          .ok()
          .map(|p| (p.id(), p.status()))
      })
      .collect();
    service_guard.stats = fresh;
  }

  Ok((provider_id, status))
}

#[tauri::command]
pub async fn get_fastest_provider(app: tauri::AppHandle) -> Result<Option<String>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;
  let fastest = service_guard.api_client.fastest_available();
  Ok(fastest.first().map(|(id, _)| id.to_string()))
}

/// Ping a single provider by id. Updates its live status and service.stats.
#[tauri::command]
pub async fn ping_api_provider(app: tauri::AppHandle, providerId: String) -> Result<(String, ProviderStatus), String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;

  let api_client = {
    let service_guard = state.lock().await;
    service_guard.api_client.clone()
  };

  let api = api_client.get_provider(&providerId).map_err(|e| e.to_string())?;
  let status = api.ping().await;

  // Update service.stats from live provider statuses.
  {
    let mut service_guard = state.lock().await;
    let fresh: Vec<_> = service_guard
      .api_client
      .get_provider_ids()
      .iter()
      .filter_map(|id| {
        service_guard
          .api_client
          .get_provider(id)
          .ok()
          .map(|p| (p.id(), p.status()))
      })
      .collect();
    service_guard.stats = fresh;
  }

  Ok((providerId, status))
}

#[tauri::command]
pub async fn get_launcher_bg(
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<Vec<u8>, String> {
  let (api_client, provider_id) = {
    let service_guard = service.lock().await;
    let api = service_guard.api_client.current_provider().map_err(|e| e.to_string())?;
    (service_guard.api_client.clone(), api.id().to_string())
  };
  let url = api_client.current_provider().map_err(|e| e.to_string())?.launcher_bg_url();

  // Fast path: index bg_etag matches the saved one -> serve from disk, no network.
  let index_bg_etag = crate::service::index::load_index(&provider_id)
    .await
    .ok()
    .and_then(|i| i.launcher.bg_etag);
  let saved_etag = { app_config.lock().await.bg_etag.clone() };
  if let (Some(idx), Some(saved)) = (&index_bg_etag, &saved_etag)
    && idx == saved
    && let Some(bytes) = crate::utils::http_cache::read_body(&url)
  {
    log::info!("get_launcher_bg: etag match, serving cached bg (0 network requests)");
    return Ok(bytes);
  }

  // Slow path: fetch via the ETag disk cache.
  let cached = crate::utils::http_cache::fetch(
    &crate::utils::http_cache::SHARED_CLIENT,
    &url,
    std::time::Duration::from_secs(crate::consts::CACHE_TTL_BACKGROUND_SECS),
  )
  .await
  .map_err(|e| format!("Cannot fetch launcher bg: {}", e))?;

  // Persist the served etag for the fast path next time.
  if let Some(etag) = crate::utils::http_cache::read_etag(&url) {
    let mut cfg = app_config.lock().await;
    cfg.bg_etag = Some(etag);
    let _ = cfg.save();
  }

  Ok(cached.bytes)
}

#[tauri::command]
pub async fn set_token_for_provider(app: tauri::AppHandle, token: String, providerId: String) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;
  let provider = match service_guard.api_client.get_provider(&providerId) {
    Ok(p) => p,
    Err(e) => {
      let msg = format!("Cannot get api provider by id {}, error: {:?}", &providerId, e);
      log::error!("{}", msg);

      return Err(msg);
    }
  };

  if let Err(e) = provider.set_token(token.clone()) {
    let msg = format!("Cannot set token for api provider by id {}, error: {:?}", &providerId, e);
    log::error!("{}", msg);

    return Err(msg);
  }

  let encoded_token = encode_token(&token);
  log::info!("set_token_for_provider: id: {}", &providerId);
  {
    let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("AppConfig not initialized")?;
    let mut service_guard = state.lock().await;

    service_guard.tokens.insert(providerId, encoded_token);
    service_guard.save().map_err(|e| e.to_string())?;
    log::info!("Save set_token_for_provider");
  }

  Ok(())
}

#[tauri::command]
pub async fn get_provider_ids(app: tauri::AppHandle) -> Result<Vec<String>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;

  Ok(service_guard.api_client.get_provider_ids())
}

#[tauri::command]
pub async fn check_available_disk_space(path: String, needed: u64) -> Result<bool, String> {
  let path = Path::new(&path);
  let bytes = fs4::available_space(path).map_err(|e| e.to_string())?;

  if bytes > needed {
    return Ok(true);
  }

  Ok(false)
}

#[tauri::command]
pub async fn remove_download_version(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>, versionName: String) -> Result<(), String> {
  let version = {
    let cfg = app_config.lock().await;
    cfg
      .progress_download
      .get(&versionName)
      .cloned()
      .ok_or_else(|| format!("remove_download_version() version not found: {} !", &versionName))?
  };

  // The download dir may already be absent (already removed in a prior run, or
  // cleared by the user/OS). Cleanup is best-effort: treat NotFound as success
  // instead of bubbling os error 3 up to the frontend, which previously aborted
  // the post-unpack sequence (clear_progress_version never ran, UI hung).
  match fs::remove_dir_all(Path::new(&version.download_path)) {
    Ok(_) => {}
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
      log::warn!("remove_download_version: dir already absent: {}", &version.download_path);
    }
    Err(e) => return Err(e.to_string()),
  }

  {
    let mut cfg = app_config.lock().await;
    cfg.progress_download.remove(&versionName);
    cfg.save().map_err(|e| e.to_string())?;
  }

  Ok(())
}

// Cancel-only cleanup of the partial install dir (where archives were being
// unpacked). Reads installed_path from progress_download, so it must be called
// BEFORE remove_download_version/clear_progress_version (which delete that
// entry). Best-effort: tolerate NotFound. NOTE: remove_download_version must
// NOT remove the install dir — it is also called after a successful unpack,
// where the install dir is the installed game.
#[tauri::command]
pub async fn remove_install_dir(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>, versionName: String) -> Result<(), String> {
  let install_path = {
    let cfg = app_config.lock().await;
    cfg
      .progress_download
      .get(&versionName)
      .map(|v| v.installed_path.clone())
      .ok_or_else(|| format!("remove_install_dir: version not found: {}", &versionName))?
  };

  match fs::remove_dir_all(Path::new(&install_path)) {
    Ok(_) => {}
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
      log::warn!("remove_install_dir: dir already absent: {}", &install_path);
    }
    Err(e) => return Err(e.to_string()),
  }

  Ok(())
}

#[tauri::command]
pub async fn move_version(
  app: tauri::AppHandle,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  versionName: String,
  dest: String,
) -> Result<(), String> {
  let version = {
    let cfg = app_config.lock().await;
    cfg
      .installed_versions
      .get(&versionName)
      .ok_or_else(|| format!("move_version() version not found: {} !", &versionName))?
      .clone()
  };

  let mut options = CopyOptions::new();
  options.overwrite = true;
  options.content_only = true;

  // Validate the IPC-provided destination BEFORE the OverwriteAll move:
  // blocks drive roots, system directories and moving a version into itself.
  crate::utils::paths::assert_move_destination(Path::new(&version.installed_path), Path::new(&dest))?;

  // Moving gigabytes is sync IO — run it on the blocking pool so other
  // commands keep responding while the move is in progress.
  let version_name_for_progress = version.name.clone();
  let app_for_progress = app.clone();
  let src = version.installed_path.clone();
  let dest_for_move = dest.clone();
  tokio::task::spawn_blocking(move || {
    move_dir_with_progress(&src, &dest_for_move, &options, move |process_info: TransitProcess| {
      // Guard division by zero for empty dirs (Bug E fix pattern).
      let percentage = if process_info.total_bytes > 0 {
        (process_info.copied_bytes as f64 / process_info.total_bytes as f64) * 100.0
      } else {
        0.0
      };

      let payload = ProgressPayload {
        version_name: version_name_for_progress.clone(),
        file_name: process_info.file_name,
        bytes_moved: process_info.copied_bytes,
        total_bytes: process_info.total_bytes,
        percentage,
      };

      let _ = app_for_progress.emit("move-version", payload);
      TransitProcessResult::OverwriteAll
    })
  })
  .await
  .map_err(|e| e.to_string())?
  .map_err(|e| e.to_string())?;

  let payload = ProgressPayload {
    version_name: version.name.clone(),
    file_name: "".to_owned(),
    bytes_moved: 0,
    total_bytes: 0,
    percentage: 100.,
  };

  let _ = app.emit("move-version", payload);

  {
    let mut cfg = app_config.lock().await;
    let v = cfg
      .installed_versions
      .get_mut(&versionName)
      .ok_or_else(|| format!("move_version() version not found: {} !", &versionName))?;

    v.installed_path = dest;
    cfg.save().map_err(|e| e.to_string())?;
  };

  Ok(())
}

/// Collect release index JSON from live API data for preview.
/// Returns the JSON string to be displayed in a text area.
#[tauri::command]
pub async fn preview_index(app: tauri::AppHandle) -> Result<String, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let api_client = {
    let service_guard = state.lock().await;
    service_guard.api_client.clone()
  };
  let api = api_client.current_provider().map_err(|e| e.to_string())?;

  crate::service::index_publisher::collect_index(api)
    .await
    .map_err(|e| {
      log::error!("preview_index failed: {:?}", e);
      e.to_string()
    })
}

/// Commit a previously previewed index JSON to the provider's index repo.
#[tauri::command]
pub async fn commit_index(app: tauri::AppHandle, json: String) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let api_client = {
    let service_guard = state.lock().await;
    service_guard.api_client.clone()
  };
  let api = api_client.current_provider().map_err(|e| e.to_string())?;

  crate::service::index_publisher::commit_index_json(api, &json)
    .await
    .map_err(|e| {
      log::error!("commit_index failed: {:?}", e);
      e.to_string()
    })
}
