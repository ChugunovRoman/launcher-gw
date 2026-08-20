use tauri::Manager;
use tauri_plugin_window_state::{AppHandleExt, StateFlags};

use crate::configs::AppConfig::AppConfig;
use crate::handlers::start_download_version::CancelMap;
use crate::handlers::upload_v2::UploadCancelMap;

/// Cancels every active download, flushes the config, then exits.
/// `app_exit` is invoked from synchronous contexts (frontend invoke), so the
/// async graceful shutdown is run via `block_on` to guarantee the config is
/// flushed before the process terminates.
#[tauri::command]
pub fn app_exit(app: tauri::AppHandle) {
  log::info!("app_exit: starting graceful shutdown");

  app.save_window_state(StateFlags::all()).unwrap_or_else(|e| {
    log::error!("app_exit: failed to save window state: {:?}", e);
  });

  // Run the async graceful shutdown on the runtime and block until it completes,
  // so the process does not exit before the config is flushed.
  tauri::async_runtime::block_on(graceful_shutdown(&app));

  log::info!("app_exit: graceful shutdown complete, exiting process");
  std::process::exit(0);
}

/// Shared graceful-shutdown routine used by `app_exit` and the window-close hook.
///
/// Signals every active download AND upload to stop, lets the workers flush their
/// `.part` files and persist partial progress into `config.json`, then performs
/// one final defensive save of the whole config. This is the fix for the bug
/// where closing the launcher via `process::exit` was dropping in-memory progress.
pub async fn graceful_shutdown(app: &tauri::AppHandle) {
  // Cancel all active downloads so workers stop writing and flush their .part files.
  if let Some(channel_map) = app.try_state::<CancelMap>() {
    let senders: Vec<tokio::sync::broadcast::Sender<()>> = {
      let map = crate::utils::locks::lock(&channel_map);
      map.iter().map(|(_, v)| v.clone()).collect()
    };

    for tx in senders {
      let _ = tx.send(());
    }
  }

  // Cancel all active uploads as well.
  if let Some(upload_map) = app.try_state::<UploadCancelMap>() {
    let senders: Vec<tokio::sync::broadcast::Sender<()>> = {
      let map = crate::utils::locks::lock(&upload_map);
      map.iter().map(|(_, v)| v.clone()).collect()
    };

    for tx in senders {
      let _ = tx.send(());
    }
  }

  // Wait until cancel maps are empty (workers finished) or timeout.
  for _ in 0..20 {
    let downloads_busy = app
      .try_state::<CancelMap>()
      .map(|m| !crate::utils::locks::lock(&m).is_empty())
      .unwrap_or(false);
    let uploads_busy = app
      .try_state::<UploadCancelMap>()
      .map(|m| !crate::utils::locks::lock(&m).is_empty())
      .unwrap_or(false);
    if !downloads_busy && !uploads_busy {
      break;
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
  }

  // Final defensive save of the whole config.
  if let Some(config_arc) = app.try_state::<std::sync::Arc<tokio::sync::Mutex<AppConfig>>>() {
    let config_guard = config_arc.lock().await;
    if let Err(e) = config_guard.save() {
      log::error!("graceful_shutdown: failed to save config: {:?}", e);
    }
  }
}
