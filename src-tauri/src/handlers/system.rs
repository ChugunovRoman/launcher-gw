use std::path::Path;

#[tauri::command]
pub fn check_file_exists(path: String) -> bool {
  Path::new(&path).exists()
}
