use anyhow::Error;
use tauri::Emitter;

pub fn log_full_error(e: &Error) {
  for cause in e.chain() {
    log::error!("Caused by: {}", cause);
  }
}

pub fn upload_log(app: &tauri::AppHandle, msg: String) {
  log::info!("{}", &msg);
  let _ = app.emit("upload-log", msg);
}
