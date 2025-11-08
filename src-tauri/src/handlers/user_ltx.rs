use std::{path::Path, sync::Arc};
use tauri::Manager;
use tokio::sync::Mutex;

use crate::{
  configs::{AppConfig::AppConfig, TmpLtx, UserLtx},
  consts::VERSIONS_DIR,
  service::{get_release::ServiceGetRelease, main::Service},
};

#[tauri::command]
pub async fn userltx_set_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;

  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let config_guard = state.lock().await;

  let releases = service_guard.get_local_version().await.map_err(|e| e.to_string())?;

  let version_path_str = releases
    .iter()
    .find(|r| r.path == path)
    .map(|r| r.path.clone())
    .ok_or_else(|| format!("Local version not found ! By path: {}", path))?;

  let state_config = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("AppConfig not initialized")?;
  let config_guard = state_config.lock().await;

  let state_userltx = app.try_state::<Arc<Mutex<UserLtx>>>().ok_or("UserLtx config not initialized")?;
  let mut userltx_guard = state_userltx.lock().await;

  let state_tmpltx = app.try_state::<Arc<Mutex<TmpLtx>>>().ok_or("TmpLtx config not initialized")?;
  let mut tmpltx_guard = state_tmpltx.lock().await;

  let version_path = Path::new(&config_guard.install_path).join(VERSIONS_DIR).join(version_path_str);

  userltx_guard.0.set_file_path(&version_path.join("appdata").join("user.ltx"));
  tmpltx_guard.0.set_file_path(version_path.join("appdata").join("user.ltx"));

  Ok(())
}
