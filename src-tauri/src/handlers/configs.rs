use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

use crate::{configs::AppConfig::AppConfig, configs::RunParams};

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
