use std::{path::Path, sync::Arc};
use tauri::Manager;
use tokio::sync::Mutex;

use crate::{
  configs::{TmpLtx, UserLtx},
  consts::*,
  service::{get_release::ServiceGetRelease, main::Service},
};

#[tauri::command]
pub async fn userltx_set_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;

  let releases = service_guard.get_local_version().await.map_err(|e| e.to_string())?;

  let installed_path = releases
    .iter()
    .find(|r| r.path == path)
    .map(|r| r.installed_path.clone())
    .ok_or_else(|| format!("Local version not found ! By path: {}", path))?;

  let state_userltx = app.try_state::<Arc<Mutex<UserLtx>>>().ok_or("UserLtx config not initialized")?;
  let mut userltx_guard = state_userltx.lock().await;

  let state_tmpltx = app.try_state::<Arc<Mutex<TmpLtx>>>().ok_or("TmpLtx config not initialized")?;
  let mut tmpltx_guard = state_tmpltx.lock().await;

  let version_path = Path::new(&installed_path);

  userltx_guard.0.set_file_path(&version_path.join(APPDATA_DIR).join(USER_LTX));
  tmpltx_guard.0.set_file_path(version_path.join(APPDATA_DIR).join(TMP_LTX));

  Ok(())
}
