mod configs;
mod consts;
mod handlers;
mod logger;
mod providers;
mod service;
mod setup;
mod utils;

use logger::Logger;
use std::sync::{Arc, Mutex};
use tauri::{Builder, Manager, Wry};

use crate::logger::TauriLogger;

fn create_tauri_app() -> Builder<Wry> {
  let mut app = tauri::Builder::default()
    .plugin(tauri_plugin_clipboard_manager::init())
    .plugin(tauri_plugin_window_state::Builder::new().build());

  app = handlers::register::register_handlers(app);

  return app;
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  let logger = Logger::new(logger::LogLevel::Debug).unwrap();
  let logger_arc = Arc::new(Mutex::new(logger));

  // Устанавливаем глобальный panic hook
  setup::setup_panic_logger(logger_arc.clone());

  let boxed = Box::new(TauriLogger { inner: logger_arc.clone() });
  log::set_boxed_logger(boxed).unwrap();
  log::set_max_level(log::LevelFilter::Trace);

  create_tauri_app()
    .setup(|app| {
      setup::tauri_setup(app)?;

      app.manage(logger_arc);

      Ok(())
    })
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_dialog::init())
    .plugin(tauri_plugin_window_state::Builder::default().build())
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
