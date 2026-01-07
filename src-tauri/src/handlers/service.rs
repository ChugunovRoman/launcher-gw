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
  let service_guard = state.lock().await;

  let results = service_guard
    .api_client
    .ping_all()
    .await
    .into_iter()
    .map(|(id, status)| (id.to_string(), status))
    .collect();
  Ok(results)
}
#[tauri::command]
pub async fn ping_current_provider(app: tauri::AppHandle) -> Result<(String, ProviderStatus), String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;

  let api = service_guard.api_client.current_provider().map_err(|e| e.to_string())?;

  let status = api.ping().await;

  Ok((api.id().to_owned(), status))
}

#[tauri::command]
pub async fn get_fastest_provider(app: tauri::AppHandle) -> Result<Option<String>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;
  let fastest = service_guard.api_client.fastest_available();
  Ok(fastest.first().map(|(id, _)| id.to_string()))
}

#[tauri::command]
pub async fn get_launcher_bg(
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  service_files: tauri::State<'_, Arc<ServiceFiles>>,
) -> Result<Vec<u8>, String> {
  let api_client = {
    let service_guard = service.lock().await;
    service_guard.api_client.clone()
  };

  let bg = match service_files.get_launcher_bg(&api_client).await {
    Ok(bytes) => bytes,
    Err(e) => {
      let msg = format!("Cannot get launcher bg, error: {:?}", e);
      log::error!("{}", msg);

      return Err(msg);
    }
  };

  Ok(bg)
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

  let encoded_token = encode(&token);
  log::info!("set_token_for_provider: id: {} encoded_token: {}", &providerId, &encoded_token);
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
      .expect(&format!("remove_download_version() version not found: {} !", &versionName))
      .clone()
  };

  let _ = fs::remove_dir_all(Path::new(&version.download_path)).map_err(|e| e.to_string())?;

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
      .expect(&format!("move_version() version not found: {} !", &versionName))
      .clone()
  };

  let mut options = CopyOptions::new();
  options.overwrite = true;
  options.content_only = true;

  let _ = move_dir_with_progress(&version.installed_path, &dest, &options, |process_info: TransitProcess| {
    let percentage = (process_info.copied_bytes as f64 / process_info.total_bytes as f64) * 100.0;

    let payload = ProgressPayload {
      version_name: version.name.clone(),
      file_name: process_info.file_name,
      bytes_moved: process_info.copied_bytes,
      total_bytes: process_info.total_bytes,
      percentage,
    };

    let _ = app.emit("move-version", payload);
    TransitProcessResult::OverwriteAll
  })
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
      .expect(&format!("move_version() version not found: {} !", &versionName));

    v.installed_path = dest;
    cfg.save().map_err(|e| e.to_string())?;
  };

  Ok(())
}
