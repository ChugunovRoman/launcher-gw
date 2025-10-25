use crate::gitlab::{Gitlab::Gitlab, files::GitLabFiles};
use std::sync::{Arc, Mutex};

#[tauri::command]
pub async fn gl_get_bg(gl: tauri::State<'_, Arc<Mutex<Gitlab>>>) -> Result<Vec<u8>, String> {
  let client = {
    let guard = gl.lock().map_err(|_| "Lock failed".to_string())?;
    guard.clone()
  };
  client
    .get_launcher_bg()
    .await
    .map_err(|e| format!("GitLab API error: {}", e))
}
