use anyhow::{Result, bail};
use std::path::PathBuf;
use tauri::{Manager, path::BaseDirectory};

pub fn get_sevenz_path(app_handle: &tauri::AppHandle) -> Result<PathBuf> {
  let binary_name = if cfg!(windows) { "7za.exe" } else { "7zzs" };

  let path = app_handle.path().resolve(binary_name, BaseDirectory::Resource)?;

  // Проверка существования (полезно при отладке)
  if !path.exists() {
    bail!(format!("7zz not found at: {:?}", path))
  }

  Ok(path)
}
