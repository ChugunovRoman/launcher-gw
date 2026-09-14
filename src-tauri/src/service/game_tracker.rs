//! Single source of truth for "is the game running" state.
//!
//! The launcher always spawns the engine directly and tracks the exact process
//! it created, identified by the `(pid, start_time, exe_path)` triple: a pid
//! alone is not stable across OS sessions because Windows aggressively reuses
//! pids (an old game pid can end up owned by svchost or the launcher's own
//! WebView2 child). Pid + process start time is unique within one OS boot
//! session, so a persisted record whose start time predates the boot time is
//! provably dead without touching the process list.

use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde::Serialize;
use sysinfo::Pid;
use sysinfo::ProcessRefreshKind;
use sysinfo::ProcessesToUpdate;
use sysinfo::System;
use sysinfo::UpdateKind;
use tauri::Emitter;
use tokio::sync::Mutex;

use crate::configs::AppConfig::AppConfig;

/// sysinfo reports start times in whole seconds; allow a small rounding delta.
const START_TIME_TOLERANCE_SECS: u64 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackedGame {
  pub pid: u32,
  /// `sysinfo Process::start_time()` in unix seconds; 0 = could not be read.
  pub start_time: u64,
  /// Real process exe path, when it could be read.
  pub exe_path: Option<String>,
  /// `Version.name`, for the frontend "In game" badge.
  pub version_name: String,
  /// subst drive letter to unmount after the game exits.
  pub subst_drive: Option<char>,
  /// `true` while the launcher is preparing user.ltx and spawning the engine.
  /// The watcher must not probe a `pid: 0` placeholder — doing so clears the
  /// claim within one second, defeating the double-launch guard (R5 fix).
  #[serde(default)]
  pub launching: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameStatus {
  pub running: bool,
  pub pid: Option<u32>,
  pub version_name: Option<String>,
}

impl GameStatus {
  pub fn idle() -> Self {
    Self { running: false, pid: None, version_name: None }
  }
}

pub struct GameTracker {
  state: Mutex<Option<TrackedGame>>,
}

impl GameTracker {
  pub fn new() -> Self {
    Self { state: Mutex::new(None) }
  }

  pub async fn set(&self, game: TrackedGame) {
    *self.state.lock().await = Some(game);
  }

  pub async fn clear(&self) {
    *self.state.lock().await = None;
  }

  pub async fn status(&self) -> GameStatus {
    match self.state.lock().await.as_ref() {
      Some(game) => GameStatus {
        running: true,
        pid: Some(game.pid),
        version_name: Some(game.version_name.clone()),
      },
      None => GameStatus::idle(),
    }
  }

  /// Atomically check that no game is running and reserve the tracker slot.
  /// Returns `true` if the claim succeeded (caller must later call `set` with
  /// the real process, or `clear` on failure). Returns `false` if a game is
  /// already tracked — the caller must not launch.
  pub async fn try_claim(&self) -> bool {
    let mut guard = self.state.lock().await;
    if guard.is_some() {
      return false;
    }
    // Insert a placeholder so concurrent callers see "running" immediately.
    // `launching: true` tells the watcher to skip probing — pid 0 is not a
    // real process; the watcher would immediately clear the claim otherwise.
    *guard = Some(TrackedGame {
      pid: 0,
      version_name: String::new(),
      start_time: 0,
      exe_path: None,
      subst_drive: None,
      launching: true,
    });
    true
  }
}

impl Default for GameTracker {
  fn default() -> Self {
    Self::new()
  }
}

/// Read `start_time` and `exe` of a freshly spawned process.
pub fn snapshot_process(pid: u32) -> Option<(u64, Option<PathBuf>)> {
  let mut system = System::new();
  let sys_pid = Pid::from_u32(pid);
  system.refresh_processes_specifics(
    ProcessesToUpdate::Some(&[sys_pid]),
    true,
    ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
  );
  system.process(sys_pid).map(|proc| (proc.start_time(), proc.exe().map(Path::to_path_buf)))
}

fn paths_equal(saved: &str, current: &Path) -> bool {
  let current = current.to_string_lossy();
  #[cfg(target_os = "windows")]
  {
    saved.eq_ignore_ascii_case(&current)
  }
  #[cfg(not(target_os = "windows"))]
  {
    saved == current
  }
}

/// Liveness rule for a tracked game (see the module docs):
/// - a saved start time older than the OS boot time means the record is dead;
/// - otherwise the pid must exist, the start time must match (±2s) when both
///   sides have a real value, and the exe path must match when both sides
///   have it;
/// - if either start time is 0 (handle could not be opened), fall back to the
///   exe path, and to "alive by pid alone" with a warning as the last resort.
pub fn probe(game: &TrackedGame) -> bool {
  // Records from a previous OS session can never be alive: pids are handed out
  // anew after reboot. Checked before any process scan.
  if game.start_time != 0 && game.start_time < System::boot_time() {
    log::info!(
      "game_tracker: pid {} start_time {} predates boot_time {}; record is dead",
      game.pid,
      game.start_time,
      System::boot_time()
    );
    return false;
  }

  let mut system = System::new();
  let sys_pid = Pid::from_u32(game.pid);
  system.refresh_processes_specifics(
    ProcessesToUpdate::Some(&[sys_pid]),
    true,
    ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
  );
  let Some(proc) = system.process(sys_pid) else {
    return false;
  };

  if game.start_time != 0 && proc.start_time() != 0 {
    if game.start_time.abs_diff(proc.start_time()) > START_TIME_TOLERANCE_SECS {
      // The pid exists but it is a different process (pid reuse).
      return false;
    }
    return match (game.exe_path.as_deref(), proc.exe()) {
      (Some(saved), Some(current)) => paths_equal(saved, current),
      _ => true,
    };
  }

  // Start time unavailable on one side: verify by exe path when possible.
  if let (Some(saved), Some(current)) = (game.exe_path.as_deref(), proc.exe()) {
    return paths_equal(saved, current);
  }

  log::warn!(
    "game_tracker: pid {} is alive but start_time/exe are unavailable; matching by pid only",
    game.pid
  );
  true
}

/// Unmount the subst drive left behind by a tracked game, if any.
fn remove_subst_drive(drive: char) {
  #[cfg(target_os = "windows")]
  {
    log::info!("game_tracker: removing subst drive {}:", drive);
    crate::handlers::process::subst_workaround::remove(drive);
  }
  #[cfg(not(target_os = "windows"))]
  {
    let _ = drive;
  }
}

/// Background watcher: once per second probes the tracked game, and when it
/// exits clears the tracker, persists the config, unmounts the subst drive and
/// notifies the frontend. Emits `game-status` only on state changes.
pub fn start_watcher(app: tauri::AppHandle, tracker: Arc<GameTracker>, config: Arc<Mutex<AppConfig>>) {
  tauri::async_runtime::spawn(async move {
    let mut last_emitted_running: Option<bool> = None;

    loop {
      tokio::time::sleep(std::time::Duration::from_secs(1)).await;

      let game = tracker.state.lock().await.clone();
      let Some(game) = game else {
        continue;
      };

      // Skip probing while the launcher is still preparing the launch
      // (user.ltx, subst, spawn). The placeholder has pid: 0 which would
      // immediately fail the liveness check and clear the claim (R5 fix).
      if game.launching {
        continue;
      }

      let probe_game = game.clone();
      let alive = tauri::async_runtime::spawn_blocking(move || probe(&probe_game))
        .await
        .unwrap_or(false);

      if alive {
        if last_emitted_running != Some(true) {
          last_emitted_running = Some(true);
          let _ = app.emit("game-status", tracker.status().await);
        }
        continue;
      }

      log::info!(
        "game_tracker: game '{}' (pid {}) exited; clearing record",
        game.version_name,
        game.pid
      );
      tracker.clear().await;
      if let Some(drive) = game.subst_drive {
        remove_subst_drive(drive);
      }
      {
        let mut config_guard = config.lock().await;
        config_guard.tracked_game = None;
        if let Err(e) = config_guard.save() {
          log::error!("game_tracker: failed to persist config after game exit: {}", e);
        }
      }
      last_emitted_running = Some(false);
      let _ = app.emit("game-status", GameStatus::idle());
    }
  });
}

#[cfg(test)]
mod tests {
  use super::*;

  fn tracked(pid: u32, start_time: u64, exe_path: Option<String>) -> TrackedGame {
    TrackedGame {
      pid,
      start_time,
      exe_path,
      version_name: "test".to_string(),
      subst_drive: None,
      launching: false,
    }
  }

  #[test]
  fn probe_false_when_started_before_boot() {
    // A record from a previous OS session is dead without a process scan,
    // even when the pid currently belongs to some unrelated process.
    let game = tracked(u32::MAX, System::boot_time().saturating_sub(100), None);
    assert!(!probe(&game));
  }

  #[test]
  fn probe_true_for_own_process() {
    // The launcher's own process is definitely alive: snapshot + probe must agree.
    let pid = std::process::id();
    let (start_time, exe_path) = snapshot_process(pid).expect("snapshot of own process");
    let exe = exe_path
      .map(|p| p.to_string_lossy().into_owned())
      .expect("own process exe must be readable");
    let game = tracked(pid, start_time, Some(exe));
    assert!(probe(&game));
  }

  #[test]
  fn probe_false_for_wrong_start_time() {
    // The real-world bug this guards against: a pid recycled by an unrelated
    // process WITHOUT a reboot (e.g. the dead game's pid handed to svchost or
    // the launcher's own WebView2 child). The "before boot" guard alone can't
    // catch that — both start times are >= boot_time — so this must be
    // rejected by the abs_diff mismatch check instead. Use our own live
    // process pid with its real start time pushed far outside the tolerance,
    // while keeping it >= boot_time so the earlier guard does not short-circuit.
    let pid = std::process::id();
    let (real_start_time, _) = snapshot_process(pid).expect("snapshot of own process");
    let mismatched_start_time = real_start_time + START_TIME_TOLERANCE_SECS + 100;
    let game = tracked(pid, mismatched_start_time, None);
    assert!(!probe(&game));
  }
}
