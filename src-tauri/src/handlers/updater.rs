use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::Mutex;

use crate::{
  service::{main::Service, updater::ServiceUpdater},
  utils::errors::log_full_error,
};

#[tauri::command]
pub async fn update(
  app: tauri::AppHandle,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  service_updater: tauri::State<'_, Arc<ServiceUpdater>>,
) -> Result<bool, String> {
  let api_client = {
    let service_guard = service.lock().await;
    service_guard.api_client.clone()
  };

  let current_version = app.package_info().version.to_string();
  if let Some(latest_release) = service_updater.check(&api_client, current_version).await.map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })? {
    let _ = app.emit("launcher-new-version", &latest_release.tag_name);

    service_updater
      .download_and_install(&api_client, &app, latest_release)
      .await
      .map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

    return Ok(true);
  };

  Ok(false)
}

#[tauri::command]
pub async fn restart_app(service_updater: tauri::State<'_, Arc<ServiceUpdater>>) -> Result<(), String> {
  service_updater.restart().await.map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  Ok(())
}
