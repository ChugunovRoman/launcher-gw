use std::sync::{Arc, Mutex};

use crate::logger::Logger;

#[tauri::command]
pub fn log_debug(logger: tauri::State<'_, Arc<Mutex<Logger>>>, msg: String) {
  if let Ok(lg) = logger.lock() {
    lg.debug(&msg);
  }
}

#[tauri::command]
pub fn log_info(logger: tauri::State<'_, Arc<Mutex<Logger>>>, msg: String) {
  if let Ok(lg) = logger.lock() {
    lg.info(&msg);
  }
}

#[tauri::command]
pub fn log_warn(logger: tauri::State<'_, Arc<Mutex<Logger>>>, msg: String) {
  if let Ok(lg) = logger.lock() {
    lg.warn(&msg);
  }
}

#[tauri::command]
pub fn log_error(logger: tauri::State<'_, Arc<Mutex<Logger>>>, msg: String) {
  if let Ok(lg) = logger.lock() {
    lg.error(&msg);
  }
}
