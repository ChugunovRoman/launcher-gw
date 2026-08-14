//! Restart handshake between launcher instances.
//!
//! Problem: the old flow did `Command::new(exe).spawn()` followed by an
//! immediate `process::exit(0)`. The freshly spawned instance then tried to
//! create its WebView2 while the dying instance (and its webview child
//! processes) still held the WebView2 user-data folder and config.json,
//! which could hang the new instance with a frozen "not responding" window.
//!
//! Fix: before spawning the replacement, the old instance grabs an exclusive
//! file lock and passes the lock path to the child via an env variable. The
//! child waits until the lock is free (= the old process is fully dead, the OS
//! releases the lock even on a hard kill/crash) and only then continues with
//! the normal startup (logger, webview creation).

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use fs4::fs_std::FileExt;
use tauri::Manager;

use crate::consts::BASE_DIR;

/// Env variable used to pass the lock file path to the spawned instance.
pub const RESTART_LOCK_ENV: &str = "GW_LAUNCHER_RESTART_LOCK";
/// Lock file name inside the launcher AppConfig directory.
pub const RESTART_LOCK_FILE: &str = "restart.lock";

/// How long the new instance waits for the old one to die before giving up.
const WAIT_TIMEOUT: Duration = Duration::from_secs(15);
/// Poll interval while waiting for the lock.
const WAIT_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Extra settle delay after the lock is acquired so leftover WebView2 child
/// processes of the dead instance can notice the parent is gone and exit.
const WEBVIEW_SETTLE_DELAY: Duration = Duration::from_millis(300);

fn resolve_lock_path(app_handle: &tauri::AppHandle) -> Option<PathBuf> {
  let dir = app_handle
    .path()
    .resolve(BASE_DIR, tauri::path::BaseDirectory::AppConfig)
    .ok()?;

  if let Err(e) = std::fs::create_dir_all(&dir) {
    log::error!("restart: cannot create lock dir {:?}: {}", dir, e);
    return None;
  }

  Some(dir.join(RESTART_LOCK_FILE))
}

/// Spawns a new launcher instance and exits the current one.
///
/// `original_exe` — path of the binary to spawn. When the launcher updated
/// itself via `self_replace`, the running binary has been renamed and
/// `current_exe()` points to the temp name, so the caller must pass the
/// original path captured *before* the replace. `None` falls back to
/// `current_exe()`.
///
/// This function never returns: it always terminates the process.
pub fn restart_launcher(app_handle: &tauri::AppHandle, original_exe: Option<PathBuf>) -> ! {
  // Take the restart lock and keep the file handle alive until process death;
  // the OS releases the lock automatically, even on a hard kill or crash.
  let lock_env = resolve_lock_path(app_handle).and_then(|lock_path| {
    let file = std::fs::OpenOptions::new()
      .create(true)
      .truncate(false)
      .write(true)
      .read(true)
      .open(&lock_path)
      .map_err(|e| {
        log::error!("restart: cannot open lock file {:?}: {}", lock_path, e);
        e
      })
      .ok()?;

    // Leak the handle on purpose: it must stay locked for the whole process
    // lifetime without relying on it being kept alive below.
    let boxed = Box::leak(Box::new(file));
    if let Err(e) = boxed.try_lock_exclusive() {
      // Another instance is mid-restart; proceed anyway with a warning —
      // waiting here would deadlock since we are about to exit.
      log::warn!("restart: lock is already held, spawning anyway: {}", e);
    }

    lock_path.into_os_string().into_string().ok()
  });

  let exe_path = original_exe
    .or_else(|| std::env::current_exe().ok())
    .unwrap_or_else(|| {
      log::error!("restart: cannot determine exe path, aborting restart");
      std::process::exit(1);
    });

  let mut command = Command::new(&exe_path);
  if let Some(lock_env) = &lock_env {
    command.env(RESTART_LOCK_ENV, lock_env);
  }

  match command.spawn() {
    Ok(child) => {
      // Avoid zombie entry in the process table on unix; on Windows the
      // handle is simply dropped and the child keeps running.
      drop(child);
      log::info!("restart: spawned new instance {:?}", exe_path);
    }
    Err(e) => {
      log::error!("restart: failed to spawn new instance {:?}: {}", exe_path, e);
      std::process::exit(1);
    }
  }

  std::process::exit(0);
}

/// Blocks until the previous instance (the one that spawned us) is fully dead.
///
/// No-op for a regular launch (double-click): the env variable is only set by
/// `restart_launcher` for its direct child. Must be called before any shared
/// resource (launcher.log, WebView2 user-data) is touched.
pub fn wait_for_previous_instance() {
  let lock_path = match std::env::var(RESTART_LOCK_ENV) {
    Ok(v) => PathBuf::from(v),
    Err(_) => return,
  };

  let deadline = std::time::Instant::now() + WAIT_TIMEOUT;
  let mut acquired = false;

  loop {
    if let Ok(file) = std::fs::OpenOptions::new()
      .create(true)
      .truncate(false)
      .write(true)
      .read(true)
      .open(&lock_path)
    {
      // The parent holds the lock until its process dies, so a successful
      // try_lock means the old instance is gone. Dropping the file unlocks.
      if file.try_lock_exclusive().is_ok() {
        drop(file);
        acquired = true;
        break;
      }
    }

    if std::time::Instant::now() >= deadline {
      break;
    }

    std::thread::sleep(WAIT_POLL_INTERVAL);
  }

  if acquired {
    std::thread::sleep(WEBVIEW_SETTLE_DELAY);
  } else {
    // Logger is not initialized yet, so plain stderr is all we have here.
    eprintln!(
      "wait_for_previous_instance: timed out after {:?} waiting for the previous instance, continuing anyway",
      WAIT_TIMEOUT
    );
  }
}
