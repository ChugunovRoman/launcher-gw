use tauri_plugin_window_state::{AppHandleExt, StateFlags};

#[tauri::command]
pub fn app_exit(app: tauri::AppHandle) {
  app
    .save_window_state(StateFlags::all())
    .expect("Cannot save the window state");

  std::process::exit(0)
}
