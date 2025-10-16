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
  config_guard
    .save_to_default(&app)
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_run_params(app: tauri::AppHandle, new_params: RunParams) -> Result<(), String> {
  let state = app
    .try_state::<Arc<Mutex<AppConfig>>>()
    .ok_or("Config not initialized")?;
  let mut config_guard = state.lock().map_err(|_| "Poisoned mutex")?;
  config_guard.run_params = new_params;
  Ok(())
}
