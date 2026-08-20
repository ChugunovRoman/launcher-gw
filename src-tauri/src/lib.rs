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
  // When spawned by a restarting launcher instance, wait until the previous
  // instance is fully dead before touching shared resources (launcher.log,
  // config.json, WebView2 user-data). No-op for regular launches.
  utils::restart::wait_for_previous_instance();

  let logger = Logger::new(logger::LogLevel::Debug);
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
    .on_window_event(|window, event| {
      // Graceful shutdown on window close (X button) — cancel active downloads
      // and persist config.json before the process dies. Without this, closing
      // the window while a download is running would lose all in-memory progress.
      if let tauri::WindowEvent::CloseRequested { .. } = event {
        log::info!("Window close requested: running graceful shutdown");
        tauri::async_runtime::block_on(handlers::window::graceful_shutdown(window.app_handle()));
      }
    })
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_dialog::init())
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
