use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

use crate::{
  configs::AppConfig::AppConfig,
  providers::dto::ProviderStatus,
  service::{files::Servicefiles, main::Service},
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
pub async fn get_fastest_provider(app: tauri::AppHandle) -> Result<Option<String>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;
  let fastest = service_guard.api_client.fastest_available();
  Ok(fastest.first().map(|(id, _)| id.to_string()))
}

#[tauri::command]
pub async fn get_launcher_bg(app: tauri::AppHandle) -> Result<Vec<u8>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;
  let bg = match service_guard.get_launcher_bg().await {
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
