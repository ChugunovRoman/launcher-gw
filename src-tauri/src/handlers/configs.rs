use std::sync::{Arc, Mutex};
use tauri::Manager;

use crate::{configs::AppConfig::AppConfig, configs::RunParams};

#[tauri::command]
pub fn get_config(app: tauri::AppHandle) -> Result<AppConfig, String> {
  let state = app
    .try_state::<Arc<Mutex<AppConfig>>>()
    .ok_or("Config not initialized")?;
  let config_guard = state.lock().map_err(|_| "Poisoned mutex")?;
  Ok(config_guard.clone())
}

#[tauri::command]
pub fn save_config(app: tauri::AppHandle) -> Result<(), String> {
  let state = app
    .try_state::<Arc<Mutex<AppConfig>>>()
    .ok_or("Config not initialized")?;
  let config_guard = state.lock().map_err(|_| "Poisoned mutex")?;
  config_guard.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_run_params(app: tauri::AppHandle, runParams: RunParams) -> Result<(), String> {
  let state = app
    .try_state::<Arc<Mutex<AppConfig>>>()
    .ok_or("Config not initialized")?;
  let mut config_guard = state.lock().map_err(|_| "Poisoned mutex")?;
  config_guard.run_params = runParams;
  config_guard.save();
  Ok(())
}

#[tauri::command]
pub fn get_lang(app: tauri::AppHandle) -> Result<String, String> {
  let state = app
    .try_state::<Arc<Mutex<AppConfig>>>()
    .ok_or("Config not initialized")?;
  let config_guard = state.lock().map_err(|_| "Poisoned mutex")?;
  Ok(config_guard.clone().lang)
}
#[tauri::command]
pub fn set_lang(app: tauri::AppHandle, lang: String) -> Result<(), String> {
  let state = app
    .try_state::<Arc<Mutex<AppConfig>>>()
    .ok_or("Config not initialized")?;
  let mut config_guard = state.lock().map_err(|_| "Poisoned mutex")?;
  config_guard.lang = lang;
  config_guard.save();
  Ok(())
}

#[tauri::command]
pub fn set_pack_paths(app: tauri::AppHandle, source: String, target: String) -> Result<(), String> {
  let state = app
    .try_state::<Arc<Mutex<AppConfig>>>()
    .ok_or("Config not initialized")?;
  let mut config_guard = state.lock().map_err(|_| "Poisoned mutex")?;
  config_guard.pack_source_dir = source;
  config_guard.pack_target_dir = target;
  config_guard.save();
  Ok(())
}
#[tauri::command]
pub fn set_unpack_paths(
  app: tauri::AppHandle,
  source: String,
  target: String,
) -> Result<(), String> {
  let state = app
    .try_state::<Arc<Mutex<AppConfig>>>()
    .ok_or("Config not initialized")?;
  let mut config_guard = state.lock().map_err(|_| "Poisoned mutex")?;
  config_guard.unpack_source_dir = source;
  config_guard.unpack_target_dir = target;
  config_guard.save();
  Ok(())
}
