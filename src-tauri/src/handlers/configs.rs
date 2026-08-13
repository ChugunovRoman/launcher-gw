use std::{collections::HashMap, fs, path::Path, sync::Arc};
use tauri::Manager;
use tokio::sync::Mutex;

use crate::{
  configs::{AppConfig::AppConfig, RunParams},
  consts::MANIFEST_NAME,
  handlers,
  providers::dto::ProviderStatus,
  service::main::Service,
  utils::encoding::decode,
};

#[tauri::command]
pub async fn get_config(app: tauri::AppHandle) -> Result<AppConfig, String> {
  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let config_guard = state.lock().await;
  // Never send provider tokens to the webview via get_config.
  let mut cfg = config_guard.clone();
  cfg.tokens.clear();
  Ok(cfg)
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
  config_guard.save().map_err(|e| e.to_string())?;
  // Also patch active version user.ltx (launch will patch again for the selected game).
  handlers::user_ltx::apply_run_params_to_version_ltx(&config_guard)
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
pub async fn set_default_install_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let mut config_guard = state.lock().await;

  config_guard.default_installed_path = path;
  config_guard.save().map_err(|e| e.to_string())?;

  Ok(())
}
#[tauri::command]
pub async fn set_default_download_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let mut config_guard = state.lock().await;

  config_guard.default_download_path = path;
  config_guard.save().map_err(|e| e.to_string())?;

  Ok(())
}
#[tauri::command]
pub async fn set_current_game_version(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>, versionName: Option<String>) -> Result<(), String> {
  {
    let mut config_guard = app_config.lock().await;

    config_guard.selected_version = versionName;
    config_guard.save().map_err(|e| e.to_string())?;
  }

  Ok(())
}

#[tauri::command]
pub async fn set_current_api_provider(
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  provider: String,
) -> Result<(), String> {
  {
    let mut config_guard = app_config.lock().await;

    config_guard.selected_provider_id = Some(provider.clone());
    config_guard.save().map_err(|e| e.to_string())?;
  }

  {
    let mut service_guard = service.lock().await;
    let api_client = &mut service_guard.api_client;
    api_client.set_current_provider(&provider).map_err(|e| e.to_string())?;
  };

  Ok(())
}

#[tauri::command]
pub async fn get_api_providers_stats(service: tauri::State<'_, Arc<Mutex<Service>>>) -> Result<Vec<(&'static str, ProviderStatus)>, String> {
  let stats = {
    let service_guard = service.lock().await;
    service_guard.stats.clone()
  };

  Ok(stats)
}
