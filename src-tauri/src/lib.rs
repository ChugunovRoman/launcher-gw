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
    // Registered FIRST on purpose: this plugin's setup hook is what terminates
    // a redundant second instance, and plugin setups run in registration order
    // during `Builder::build()`. Anything registered before it would do its
    // startup work (and touch shared files) in a process that is about to die.
    .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
      // Focus the existing window when a second instance is launched.
      if let Some(win) = app.get_webview_window("main") {
        let _ = win.set_focus();
      }
    }))
    .plugin(tauri_plugin_clipboard_manager::init())
    .plugin(tauri_plugin_window_state::Builder::new().build())
    .plugin(tauri_plugin_shell::init())
    .plugin(tauri_plugin_dialog::init());

  app = handlers::register::register_handlers(app);

  return app;
}

/// Build the logger and wire it into the `log` facade.
///
/// `Logger::new` trims the log file to the last 50 sessions and appends a new
/// session header, so it must only run in a process that is actually going to
/// live: a blocked second instance used to burn one of those 50 sessions.
fn init_logging() -> Arc<Mutex<Logger>> {
  let logger_arc = Arc::new(Mutex::new(Logger::new(logger::LogLevel::Debug)));

  // Устанавливаем глобальный panic hook
  setup::setup_panic_logger(logger_arc.clone());

  let boxed = Box::new(TauriLogger { inner: logger_arc.clone() });
  let _ = log::set_boxed_logger(boxed);
  log::set_max_level(log::LevelFilter::Trace);

  logger_arc
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  // When spawned by a restarting launcher instance, wait until the previous
  // instance is fully dead before touching shared resources (launcher.log,
  // config.json, WebView2 user-data). No-op for regular launches.
  //
  // Must stay FIRST — before the logger opens launcher.log and before the
  // single-instance plugin claims its mutex, otherwise the restarted launcher
  // would race the dying one for both.
  utils::restart::wait_for_previous_instance();

  // `build()` runs the plugin setup hooks, so a second instance exits inside
  // this call — before the logger opens a session and before any window is
  // created (windows and the `.setup()` callback below run during `run()`).
  let app = create_tauri_app()
    .setup(|app| {
      if let Err(e) = setup::tauri_setup(app) {
        // Log the error but do not abort — the window should still appear
        // with default settings so the user is not left with a blank screen.
        let msg = format!("Launcher setup error (continuing with defaults): {}", e);
        log::error!("{}", msg);
        eprintln!("{}", msg);
      }

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
    .build(tauri::generate_context!());

  let app = match app {
    Ok(app) => app,
    Err(e) => {
      // The build failed (missing WebView2 runtime, broken context …). This
      // process dies either way, so spending a log session on the reason is
      // exactly what we want.
      init_logging();
      log::error!("Failed to build tauri application: {}", e);
      panic!("error while building tauri application: {}", e);
    }
  };

  // Single instance confirmed: only now is it safe to rotate launcher.log and
  // write the session header.
  let logger_arc = init_logging();
  app.manage(logger_arc);

  app.run(|_, _| {});
}
