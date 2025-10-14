use tauri::{Builder, LogicalSize, Manager, Wry};

#[tauri::command]
fn get_window_size(app: tauri::AppHandle) -> Result<(u32, u32), String> {
  let window = app
    .get_webview_window("main")
    .ok_or("Main window not found")?;
  let size = window.inner_size().map_err(|e| e.to_string())?;
  Ok((size.width, size.height))
}

#[tauri::command]
fn set_window_size(app: tauri::AppHandle, width: u32, height: u32) -> Result<(), String> {
  let window = app
    .get_webview_window("main")
    .ok_or("Main window not found")?;
  let size = LogicalSize::new(width, height);
  window.set_size(size).map_err(|e| e.to_string())?;

  Ok(())
}

#[tauri::command]
fn app_exit() {
  std::process::exit(0)
}

pub fn register_window_handlers(app: Builder<Wry>) -> Builder<Wry> {
  app.invoke_handler(tauri::generate_handler![
    get_window_size,
    set_window_size,
    app_exit
  ])
}
