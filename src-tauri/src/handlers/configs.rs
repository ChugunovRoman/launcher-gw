use std::{collections::HashMap, fs, path::Path, sync::Arc};
use tauri::Manager;
use tokio::sync::Mutex;

use crate::{
  configs::{AppConfig::AppConfig, RunParams},
  consts::MANIFEST_NAME,
  utils::{encoding::decode, git::state::RepoSyncState},
};

#[tauri::command]
pub async fn get_config(app: tauri::AppHandle) -> Result<AppConfig, String> {
  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let config_guard = state.lock().await;
  Ok(config_guard.clone())
}

#[tauri::command]
pub async fn save_config(app: tauri::AppHandle) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let config_guard = state.lock().await;
  config_guard.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_run_params(app: tauri::AppHandle, run_params: RunParams) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let mut config_guard = state.lock().await;
  config_guard.run_params = run_params;
  config_guard.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_lang(app: tauri::AppHandle) -> Result<String, String> {
  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let config_guard = state.lock().await;
  Ok(config_guard.lang.clone())
}

#[tauri::command]
pub async fn set_lang(app: tauri::AppHandle, lang: String) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let mut config_guard = state.lock().await;
  config_guard.lang = lang;
  config_guard.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_pack_paths(app: tauri::AppHandle, source: String, target: String) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let mut config_guard = state.lock().await;
  config_guard.pack_source_dir = source;
  config_guard.pack_target_dir = target;
  config_guard.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_unpack_paths(app: tauri::AppHandle, source: String, target: String) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let mut config_guard = state.lock().await;
  config_guard.unpack_source_dir = source;
  config_guard.unpack_target_dir = target;
  config_guard.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_tokens(app: tauri::AppHandle) -> Result<HashMap<String, String>, String> {
  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let config_guard = state.lock().await;

  let decoded_tokens: HashMap<String, String> = config_guard
    .tokens
    .iter()
    .map(|(key, value)| {
      if value == "" {
        return (key.clone(), value.clone());
      }

      let decoded_value = match decode(value) {
        Ok(decoded) => decoded,
        Err(_) => value.clone(),
      };
      (key.clone(), decoded_value)
    })
    .collect();

  Ok(decoded_tokens)
}

#[tauri::command]
pub async fn get_upload_manifest(app: tauri::AppHandle) -> Result<Option<RepoSyncState>, String> {
  let progress_opt = {
    let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
    let config_guard = state.lock().await;

    config_guard.progress_upload.clone()
  };

  let progress = match progress_opt {
    Some(data) => data,
    None => {
      log::info!("get_upload_manifest(), progress_upload is empty in AppConfig, just return null");
      return Ok(None);
    }
  };

  let repo_path = Path::new(&progress.path_dir).join("main_1");

  if !repo_path.exists() {
    log::info!("get_upload_manifest(), repo path: {:?} doesn't exist, just return null", &repo_path);
    return Ok(None);
  }

  let manifest_path = repo_path.join(MANIFEST_NAME);

  if !repo_path.exists() {
    log::info!(
      "get_upload_manifest(), {} file doesn't exist by path: {:?}, just return null",
      MANIFEST_NAME,
      &manifest_path
    );
    return Ok(None);
  }

  let content = fs::read_to_string(&manifest_path).map_err(|e| e.to_string())?;
  let state: RepoSyncState = serde_json::from_str(&content).map_err(|e| e.to_string())?;

  Ok(Some(state))
}
