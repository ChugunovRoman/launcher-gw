use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::Mutex;

use crate::configs::AppConfig::{AppConfig, Version};
use crate::consts::*;
use crate::service::game_tracker::{snapshot_process, GameStatus, GameTracker, TrackedGame};
use crate::service::keybind_manager::KeybindManager;
use crate::utils::resources::{game_exe, STALKER_LAUNCHER_STEMS};
use crate::utils::split_args::split_args;
use tauri::{Emitter, Manager};

#[cfg(target_os = "windows")]
pub(crate) mod subst_workaround {
  use std::path::Path;
  use std::process::Command;

  // Find a free drive letter scanning from 'Z' down to 'D' (skip A/B/C and
  // lower letters that often map to removable media). find() short-circuits,
  // so on a typical system this only touches 'Z'.
  fn find_free_drive_letter() -> Option<char> {
    ('D'..='Z').rev().find(|&letter| !Path::new(&format!("{}:\\", letter)).exists())
  }

  // Mount `target` at a free drive letter via `subst` and return that letter.
  pub fn setup_for(target: &str) -> std::io::Result<char> {
    let drive = find_free_drive_letter().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "no free drive letter available for subst"))?;
    let output = Command::new("subst").arg(format!("{}:", drive)).arg(target).output()?;
    if !output.status.success() {
      let stderr = String::from_utf8_lossy(&output.stderr);
      return Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        format!("subst {} failed: {}", drive, stderr.trim()),
      ));
    }
    Ok(drive)
  }

  // Unmount a previously created subst drive. Best-effort; errors are ignored.
  pub fn remove(drive: char) {
    let _ = Command::new("subst").arg(format!("{}:", drive)).arg("/D").output();
  }
}

/// Structured launch error: serialized to `{ code, detail }` so the frontend
/// can show a localized title per `code` and the raw `detail` below it.
#[derive(Debug, Clone)]
pub enum LaunchError {
  AlreadyRunning,
  VersionNotFound(String),
  ExeNotFound(String),
  SpawnFailed(String),
  ExitedImmediately(String),
  LtxPrepareFailed(String),
  ConfigLocked(String),
}

impl LaunchError {
  pub fn code(&self) -> &'static str {
    match self {
      LaunchError::AlreadyRunning => "already_running",
      LaunchError::VersionNotFound(_) => "version_not_found",
      LaunchError::ExeNotFound(_) => "exe_not_found",
      LaunchError::SpawnFailed(_) => "spawn_failed",
      LaunchError::ExitedImmediately(_) => "exited_immediately",
      LaunchError::LtxPrepareFailed(_) => "ltx_prepare_failed",
      LaunchError::ConfigLocked(_) => "config_locked",
    }
  }

  pub fn detail(&self) -> String {
    match self {
      LaunchError::AlreadyRunning => "Another game session is already running".to_string(),
      LaunchError::VersionNotFound(d)
      | LaunchError::ExeNotFound(d)
      | LaunchError::SpawnFailed(d)
      | LaunchError::ExitedImmediately(d)
      | LaunchError::LtxPrepareFailed(d)
      | LaunchError::ConfigLocked(d) => d.clone(),
    }
  }
}

impl Serialize for LaunchError {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    use serde::ser::SerializeStruct;
    let mut state = serializer.serialize_struct("LaunchError", 2)?;
    state.serialize_field("code", self.code())?;
    state.serialize_field("detail", &self.detail())?;
    state.end()
  }
}

/// Every launch failure is logged on the Rust side before it is returned.
fn log_launch_error(e: LaunchError) -> LaunchError {
  log::error!("run_game failed [{}]: {}", e.code(), e.detail());
  e
}

/// True when the file name of `exe_path` is one of the known Stalker launcher
/// stubs (Stalker-CoC.exe / Stalker-CoP.exe / Stalker-CS.exe / Stalker.exe).
fn is_stalker_launcher_stub(exe_path: &str) -> bool {
  let Some(file_name) = Path::new(exe_path).file_name().and_then(|n| n.to_str()) else {
    return false;
  };
  STALKER_LAUNCHER_STEMS.iter().any(|stem| {
    if cfg!(windows) {
      file_name.eq_ignore_ascii_case(&format!("{stem}.exe"))
    } else {
      file_name.eq_ignore_ascii_case(stem)
    }
  })
}

// Resolve the launch target (exe, working dir) by priority tiers. The engine is
// ALWAYS launched directly:
//   1. version.exe_path          — manifest-provided engine exe. Stalker
//                                  launcher stubs are ignored (see below)
//   2. engine_path + fsgame_path — manual engine; CWD = directory of fsgame.ltx
//   5. default bin/xrEngine.exe
//
// The Stalker-CoC.exe / CoP / CS / Stalker stubs are compiled AutoHotkey
// wrappers: they check the registry for an installed Call of Pripyat (and just
// exit with code 0 when it is missing), then ShellExecute bin\xrEngine.exe with
// RunAs — i.e. UAC elevation on every launch. An elevated engine also loses the
// launcher's subst drives (separate drive table), which is what broke installs
// in temp/rar$ paths and on machines without CoP. Launching the engine binary
// directly avoids all of it, so the stubs are never used.
pub(crate) fn resolve_launch_target(version: &Version, installed_path: &Path) -> (PathBuf, PathBuf) {
  // Tier 1: explicit exe_path (relative to installed_path; absolute also works via join)
  if let Some(exe_rel) = version.exe_path.as_ref() {
    if is_stalker_launcher_stub(exe_rel) {
      // Never launch the wrapper stubs — see the comment above.
      log::info!("launch tier 1: exe_path {:?} is a Stalker launcher stub, ignored", exe_rel);
    } else {
      let candidate = installed_path.join(exe_rel);
      if candidate.exists() {
        log::info!("launch tier 1 (exe_path): {:?}", candidate);
        return (candidate, installed_path.to_path_buf());
      }
      log::warn!("exe_path set but not found: {:?}; falling through", candidate);
    }
  }

  // Tier 2: manual engine_path + fsgame_path; CWD = directory of fsgame.ltx
  if let (Some(engine), Some(fsgame)) = (version.engine_path.as_ref(), version.fsgame_path.as_ref()) {
    let exe = PathBuf::from(engine);
    let cwd = Path::new(fsgame)
      .parent()
      .filter(|p| !p.as_os_str().is_empty())
      .map(PathBuf::from)
      .unwrap_or_else(|| installed_path.to_path_buf());
    log::info!("launch tier 2 (engine_path): exe {:?}, cwd {:?}", exe, cwd);
    return (exe, cwd);
  }

  // Tier 5: default bin/xrEngine.exe
  let exe = installed_path.join(BIN_DIR).join(game_exe());
  log::info!("launch tier 5 (default engine): {:?}", exe);
  (exe, installed_path.to_path_buf())
}

/// Resolve launch target on the backend — never trust a full Version from IPC.
///
/// Reads only `AppConfig` (already held by the caller) — no `Service` state,
/// so this never waits behind the background network task's long-held
/// `Service` lock (provider ping, index fetch, ...).
async fn resolve_version_for_launch(config: &AppConfig, version_name: Option<&str>, use_main: bool) -> Result<Version, String> {
  if use_main || version_name.is_none() {
    return crate::service::get_release::get_main_version_from_config(config)
      .await
      .ok_or_else(|| "Main game version not found next to launcher".to_string());
  }

  let name = version_name.unwrap();
  if let Some(v) = config.installed_versions.values().find(|v| v.name == name || v.path == name) {
    return Ok(v.clone());
  }
  if let Some(v) = config.installed_versions.get(name) {
    return Ok(v.clone());
  }
  if let Some(v) = config.versions.iter().find(|v| v.name == name || v.path == name) {
    if !v.installed_path.is_empty() {
      return Ok(v.clone());
    }
  }

  Err(format!("Installed version not found: {}", name))
}

#[tauri::command]
pub async fn run_game(
  app: tauri::AppHandle,
  keybind_manager: tauri::State<'_, Arc<KeybindManager>>,
  versionName: Option<String>,
  useMain: Option<bool>,
) -> Result<GameStatus, LaunchError> {
  let tracker = app
    .try_state::<Arc<GameTracker>>()
    .ok_or_else(|| log_launch_error(LaunchError::ConfigLocked("Game tracker not initialized".to_string())))?;
  let state = app
    .try_state::<Arc<Mutex<AppConfig>>>()
    .ok_or_else(|| log_launch_error(LaunchError::ConfigLocked("Config not initialized".to_string())))?;

  // One game at a time: atomically claim the tracker slot to prevent
  // concurrent launches from double-click or parallel requests.
  if !tracker.try_claim().await {
    return Err(log_launch_error(LaunchError::AlreadyRunning));
  }

  // From here on the tracker holds a `launching: true` placeholder that the
  // watcher deliberately never reaps (game_tracker.rs). Every failure path MUST
  // release it: otherwise the claim outlives the failed launch attempt and every
  // later "Play" answers AlreadyRunning until the launcher is restarted.
  // Releasing it in exactly one place here is what keeps that guarantee true for
  // future early returns as well (R10).
  let result = run_game_claimed(&app, &tracker, &state, &keybind_manager, versionName, useMain).await;
  if result.is_err() {
    tracker.clear().await;
  }
  result
}

/// Body of `run_game`, executed with the tracker slot already claimed.
/// Never call directly — the caller owns claiming and releasing the slot.
async fn run_game_claimed(
  app: &tauri::AppHandle,
  tracker: &Arc<GameTracker>,
  state: &Arc<Mutex<AppConfig>>,
  keybind_manager: &Arc<KeybindManager>,
  version_name: Option<String>,
  use_main: Option<bool>,
) -> Result<GameStatus, LaunchError> {
  // Snapshot launch-critical fields and drop the config lock right away:
  // the launch path does sync fs work (user.ltx) and process spawning —
  // holding the lock through all of that froze every other config command
  // for the whole launch sequence.
  let (version, run_params_snapshot, profile_for_launch, provider_id_for_launch) = {
    let config_guard = state.lock().await;

    let version = resolve_version_for_launch(&config_guard, version_name.as_deref(), use_main.unwrap_or(false))
      .await
      .map_err(|e| log_launch_error(LaunchError::VersionNotFound(e)))?;

    // Pre-launch only: patch launcher settings into the selected game's user.ltx.
    // Do NOT rewrite user.ltx after the game exits (engine owns saves during/after session).
    let profile_for_launch = if config_guard.should_apply_key_profile() {
      config_guard.selected_profile.clone()
    } else {
      None
    };

    (
      version,
      config_guard.run_params.clone(),
      profile_for_launch,
      config_guard.selected_provider_id.clone(),
    )
  };

  let target_path = version.installed_path.clone();

  let installed_path = PathBuf::from(&target_path);
  let (exe, cwd) = resolve_launch_target(&version, &installed_path);
  let user_ltx_path = match &version.userltx_path {
    Some(value) => Path::new(value).to_path_buf(),
    None => Path::new(&target_path).join(APPDATA_DIR).join(USER_LTX),
  };
  let tmp_ltx_path = Path::new(&target_path).join(APPDATA_DIR).join(TMP_LTX);
  let alife_ltx_path = crate::handlers::user_ltx::alife_ltx_path_in(&cwd);

  crate::handlers::user_ltx::prepare_ltx_for_launch(
    &user_ltx_path,
    &tmp_ltx_path,
    &alife_ltx_path,
    &run_params_snapshot,
    provider_id_for_launch.as_deref(),
    &keybind_manager,
    profile_for_launch.as_deref(),
  )
  .await
  .map_err(|e| log_launch_error(LaunchError::LtxPrepareFailed(e)))?;

  // Fail early with a clear code when the engine binary is missing (e.g. a
  // broken install) instead of a cryptic spawn failure.
  if !exe.is_file() {
    return Err(log_launch_error(LaunchError::ExeNotFound(exe.to_string_lossy().into_owned())));
  }

  // Do NOT pass -fsltx: the engine resolves fsgame.ltx relative to the current
  // working directory (current_dir = game root below). That works with Cyrillic
  // and spaces in the path, unlike -fsltx whose value is parsed with
  // sscanf("%[^ ] ") (truncated at the first space) from an ANSI command line
  // and then decoded as UTF-8 — both break non-ASCII/spaced paths.
  let mut run_params: Vec<String> = Vec::new();

  if run_params_snapshot.check_no_staging {
    run_params.push("-no_staging".to_string());
  }
  if run_params_snapshot.check_spawner {
    run_params.push("-dbg".to_string());
  }
  if run_params_snapshot.check_without_cache {
    run_params.push("-noprefetch".to_string());
  }
  if run_params_snapshot.checks {
    run_params.push("-checks".to_string());
  }
  if run_params_snapshot.ui_debug {
    run_params.push("-uidbg".to_string());
  }
  if run_params_snapshot.debug_spawn {
    run_params.push("-dbgsspwn".to_string());
  }
  let users_args = split_args(&run_params_snapshot.cmd_params);
  run_params.extend(users_args);

  // Engine flags (-dbg, -uidbg, ...) exist ONLY on the command line — they
  // cannot be expressed via user.ltx.
  log::info!("Start game exe: {:?} with params: {:?} target_path: {:?}", &exe, &run_params, target_path);

  // Direct engine launches resolve $fs_root$ from the CWD via the ANSI Win32
  // API and decode it as UTF-8, so a non-ASCII CWD corrupts it. Hide a
  // non-ASCII CWD behind a virtual drive (subst) so the engine only sees ASCII.
  // Now applied to every launch because every launch is a direct engine launch.
  #[cfg(target_os = "windows")]
  let (effective_cwd, subst_drive): (PathBuf, Option<char>) = {
    let cwd_str = cwd.to_string_lossy();
    if !cwd_str.is_ascii() {
      match subst_workaround::setup_for(&cwd_str) {
        Ok(drive) => {
          log::info!("subst: mounted non-ASCII CWD '{}' to {}:", cwd_str, drive);
          (PathBuf::from(format!("{}:\\", drive)), Some(drive))
        }
        Err(e) => {
          log::error!(
            "subst workaround failed for '{}': {}. Launching with real path (Cyrillic may fail inside engine).",
            cwd_str,
            e
          );
          (cwd.clone(), None)
        }
      }
    } else {
      (cwd.clone(), None)
    }
  };
  #[cfg(not(target_os = "windows"))]
  let (effective_cwd, subst_drive): (PathBuf, Option<char>) = (cwd.clone(), None);

  // When a subst drive is active, launch the exe THROUGH it (e.g. Z:\bin\xrEngine.exe)
  // so the launched process sees only ASCII paths via GetModuleFileName and the
  // inherited CWD. The subst drive is unmounted by the game tracker watcher
  // after the tracked process exits.
  let launch_exe = match subst_drive {
    Some(drive) => exe
      .strip_prefix(&cwd)
      .map(|rel| PathBuf::from(format!("{}:\\", drive)).join(rel))
      .unwrap_or_else(|_| exe.clone()),
    None => exe.clone(),
  };

  log::info!(
    "run_game exe: {:?}, CWD: {:?} (subst: {})",
    &launch_exe,
    &effective_cwd,
    subst_drive.is_some()
  );

  let child = match Command::new(&launch_exe)
    .args(&run_params)
    .current_dir(&effective_cwd)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
  {
    Ok(child) => child,
    Err(e) => {
      #[cfg(target_os = "windows")]
      if let Some(drive) = subst_drive {
        subst_workaround::remove(drive);
      }
      return Err(log_launch_error(LaunchError::SpawnFailed(format!("{} ({})", e, exe.display()))));
    }
  };

  // The spawned process IS the engine (no wrapper in between). Detach it fully:
  // the game must be independent of the launcher lifecycle.
  let pid = child.id();
  drop(child);

  // Snapshot (start_time, exe) right after spawn — the identity of the tracked
  // process for all later liveness probes.
  let snapshot = match tauri::async_runtime::spawn_blocking(move || snapshot_process(pid)).await {
    Ok(s) => s,
    Err(e) => {
      #[cfg(target_os = "windows")]
      if let Some(drive) = subst_drive {
        subst_workaround::remove(drive);
      }
      return Err(log_launch_error(LaunchError::SpawnFailed(format!("snapshot task failed: {}", e))));
    }
  };

  let Some((start_time, exe_snapshot)) = snapshot else {
    // Process died between spawn and snapshot — the engine failed to start.
    #[cfg(target_os = "windows")]
    if let Some(drive) = subst_drive {
      subst_workaround::remove(drive);
    }
    return Err(log_launch_error(LaunchError::ExitedImmediately(format!("pid {}", pid))));
  };

  let tracked = TrackedGame {
    pid,
    start_time,
    exe_path: exe_snapshot.map(|p| p.to_string_lossy().into_owned()),
    version_name: version.name.clone(),
    subst_drive,
    launching: false,
  };

  tracker.set(tracked.clone()).await;

  {
    let mut config_guard = state.lock().await;
    config_guard.tracked_game = Some(tracked);
    // The game is already running — a config save failure must NOT be reported
    // as a launch failure.
    if let Err(e) = config_guard.save() {
      log::error!("run_game: failed to persist tracked_game into config: {}", e);
    }
  }

  let status = tracker.status().await;
  let _ = app.emit("game-status", &status);

  Ok(status)
}

#[tauri::command]
pub fn get_passed_args() -> Vec<String> {
  let args: Vec<String> = std::env::args().skip(1).collect();
  log::info!("Passed args: {:?}", args);
  args
}

#[tauri::command]
pub async fn get_game_status(app: tauri::AppHandle) -> Result<GameStatus, String> {
  let tracker = app
    .try_state::<Arc<GameTracker>>()
    .ok_or_else(|| "Game tracker not initialized".to_string())?;
  Ok(tracker.status().await)
}

/// Warning codes for install paths in temp directories (see `utils::paths`).
#[tauri::command]
pub fn check_install_path(path: String) -> Vec<String> {
  crate::utils::paths::is_temp_path(Path::new(&path)).into_iter().map(str::to_string).collect()
}

#[tauri::command]
pub fn open_explorer(path: String, createDir: Option<bool>) -> Result<(), String> {
  let p = Path::new(&path);
  // Guard the IPC-driven directory creation/open: absolute paths only, no
  // drive roots or system locations. For new paths the parent must exist.
  crate::utils::paths::assert_creatable_directory(p)?;
  if !p.exists() {
    if createDir.unwrap_or(false) {
      std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
    } else {
      return Err(format!("Path does not exist: {}", path));
    }
  }

  #[cfg(target_os = "windows")]
  {
    Command::new("explorer").arg(path).spawn().map_err(|e| e.to_string())?;
  }

  #[cfg(target_os = "macos")]
  {
    Command::new("open").arg(path).spawn().map_err(|e| e.to_string())?;
  }

  #[cfg(target_os = "linux")]
  {
    Command::new("xdg-open").arg(path).spawn().map_err(|e| e.to_string())?;
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::is_stalker_launcher_stub;

  #[test]
  fn stalker_launcher_stub_filter() {
    // Known wrapper stubs must be ignored in any directory/case.
    assert!(is_stalker_launcher_stub("Stalker-CoC.exe"));
    assert!(is_stalker_launcher_stub("bin\\Stalker-CoP.exe"));
    assert!(is_stalker_launcher_stub("Stalker-CS.EXE"));
    assert!(is_stalker_launcher_stub("Stalker.exe"));
    // Real engine binaries and custom engine names pass through.
    assert!(!is_stalker_launcher_stub("bin/xrEngine.exe"));
    assert!(!is_stalker_launcher_stub("MyModEngine.exe"));
    assert!(!is_stalker_launcher_stub(""));
  }
}
