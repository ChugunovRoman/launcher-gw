use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tokio::sync::Mutex;

use crate::configs::AppConfig::{AppConfig, Version};
use crate::consts::*;
use crate::service::get_release::ServiceGetRelease;
use crate::service::keybind_manager::KeybindManager;
use crate::utils::errors::log_full_error;
use crate::utils::resources::game_exe;
use crate::utils::split_args::split_args;
use tauri::Manager;

#[cfg(target_os = "windows")]
mod subst_workaround {
  use std::path::Path;
  use std::process::Command;

  // Find a free drive letter scanning from 'Z' down to 'D' (skip A/B/C and
  // lower letters that often map to removable media). find() short-circuits,
  // so on a typical system this only touches 'Z'.
  fn find_free_drive_letter() -> Option<char> {
    ('D'..='Z')
      .rev()
      .find(|&letter| !Path::new(&format!("{}:\\", letter)).exists())
  }

  // Mount `target` at a free drive letter via `subst` and return that letter.
  pub fn setup_for(target: &str) -> std::io::Result<char> {
    let drive = find_free_drive_letter().ok_or_else(|| {
      std::io::Error::new(std::io::ErrorKind::Other, "no free drive letter available for subst")
    })?;
    let output = Command::new("subst")
      .arg(format!("{}:", drive))
      .arg(target)
      .output()?;
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
    let _ = Command::new("subst")
      .arg(format!("{}:", drive))
      .arg("/D")
      .output();
  }
}

// Resolve the launch target (exe, working dir, is_xray_engine) by priority tiers:
//   1. version.exe_path          — explicit launcher exe (e.g. Stalker-CoC.exe from manifest)
//   2. engine_path + fsgame_path — manual engine; CWD = directory of fsgame.ltx
//   3-4. auto-detect Stalker-CoC/CoP/CS/Stalker.exe in installed_path
//   5. default bin/xrEngine.exe
// `is_xray_engine` gates the subst CWD workaround: only the xray engine has the
// ANSI-CWD bug; Stalker launcher exes manage their own CWD.
fn resolve_launch_target(version: &Version, installed_path: &Path) -> (PathBuf, PathBuf, bool) {
  // Tier 1: explicit exe_path (relative to installed_path; absolute also works via join)
  if let Some(exe_rel) = version.exe_path.as_ref() {
    let candidate = installed_path.join(exe_rel);
    if candidate.exists() {
      log::info!("launch tier 1 (exe_path): {:?}", candidate);
      return (candidate, installed_path.to_path_buf(), false);
    }
    log::warn!("exe_path set but not found: {:?}; falling through", candidate);
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
    return (exe, cwd, true);
  }

  // Tiers 3-4: auto-detect Stalker-CoC/CoP/CS/Stalker in installed_path.
  // ASCII-only: Stalker-CoC.exe is a wrapper that starts the engine itself and
  // handles non-ASCII install paths under manual double-click. But launched
  // from the launcher it exits with code 0 on a Cyrillic path (unsolved why —
  // double-click works, launcher does not), so for non-ASCII paths we skip it
  // and fall through to the direct xrEngine + subst tier, which handles Cyrillic.
  if installed_path.to_string_lossy().is_ascii() {
    if let Some(launcher) = crate::utils::resources::find_stalker_launcher(installed_path) {
      log::info!("launch tier 3/4 (stalker launcher): {:?}", launcher);
      return (launcher, installed_path.to_path_buf(), false);
    }
  }

  // Tier 5: default bin/xrEngine.exe
  let exe = installed_path.join(BIN_DIR).join(game_exe());
  log::info!("launch tier 5 (default engine): {:?}", exe);
  (exe, installed_path.to_path_buf(), true)
}

/// Resolve launch target on the backend — never trust a full Version from IPC.
async fn resolve_version_for_launch(
  app: &tauri::AppHandle,
  config: &AppConfig,
  version_name: Option<&str>,
  use_main: bool,
) -> Result<Version, String> {
  if use_main || version_name.is_none() {
    let service = app
      .try_state::<Arc<Mutex<crate::service::main::Service>>>()
      .ok_or("Service not initialized")?;
    let service_guard = service.lock().await;
    return service_guard
      .get_main_version()
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

/// Find xrEngine PID after spawning a Stalker-* wrapper (exits quickly).
/// Prefer child of `wrapper_pid`; else match exe under game roots.
fn resolve_engine_pid(wrapper_pid: u32, is_xray_engine: bool, cwd: &Path, installed_path: &Path) -> u32 {
  if is_xray_engine {
    return wrapper_pid;
  }

  let engine_name = game_exe();
  let expected_under_cwd = cwd.join(BIN_DIR).join(&engine_name);
  let expected_under_install = installed_path.join(BIN_DIR).join(&engine_name);
  let wrapper = Pid::from(wrapper_pid as usize);

  let refresh = ProcessRefreshKind::nothing()
    .with_exe(UpdateKind::OnlyIfNotSet)
    .with_cmd(UpdateKind::OnlyIfNotSet);

  for _ in 0..40 {
    let mut system = System::new();
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, refresh);

    let mut path_matches: Vec<u32> = Vec::new();

    for (pid, proc) in system.processes() {
      let name = proc.name().to_string_lossy();
      if !name.eq_ignore_ascii_case(&engine_name) {
        continue;
      }

      if proc.parent() == Some(wrapper) {
        log::info!("resolve_engine_pid: child of wrapper {} -> {}", wrapper_pid, pid.as_u32());
        return pid.as_u32();
      }

      if let Some(exe) = proc.exe() {
        if exe == expected_under_cwd.as_path() || exe == expected_under_install.as_path() {
          path_matches.push(pid.as_u32());
        }
      }
    }

    if let Some(pid) = path_matches.into_iter().next() {
      log::info!("resolve_engine_pid: matched by exact path -> {}", pid);
      return pid;
    }

    std::thread::sleep(Duration::from_millis(50));
  }

  log::warn!(
    "resolve_engine_pid: engine not found; falling back to wrapper pid {}",
    wrapper_pid
  );
  wrapper_pid
}

#[tauri::command]
pub async fn run_game(
  app: tauri::AppHandle,
  keybind_manager: tauri::State<'_, Arc<KeybindManager>>,
  versionName: Option<String>,
  useMain: Option<bool>,
) -> Result<u32, String> {
  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let mut config_guard = state.lock().await;

  let version = resolve_version_for_launch(&app, &config_guard, versionName.as_deref(), useMain.unwrap_or(false)).await?;

  let target_path = version.installed_path.clone();

  let installed_path = PathBuf::from(&target_path);
  let (exe, cwd, is_xray_engine) = resolve_launch_target(&version, &installed_path);
  let user_ltx_path = match &version.userltx_path {
    Some(value) => Path::new(value).to_path_buf(),
    None => Path::new(&target_path).join(APPDATA_DIR).join(USER_LTX),
  };
  let tmp_ltx_path = Path::new(&target_path).join(APPDATA_DIR).join(TMP_LTX);

  // Pre-launch only: patch launcher settings into the selected game's user.ltx.
  // Do NOT rewrite user.ltx after the game exits (engine owns saves during/after session).
  let profile_for_launch = if config_guard.should_apply_key_profile() {
    config_guard.selected_profile.as_deref()
  } else {
    None
  };
  crate::handlers::user_ltx::prepare_ltx_for_launch(
    &user_ltx_path,
    &tmp_ltx_path,
    &config_guard.run_params,
    &keybind_manager,
    profile_for_launch,
  )
  .await?;

  // Do NOT pass -fsltx: the engine resolves fsgame.ltx relative to the current
  // working directory (current_dir = game root below). That works with Cyrillic
  // and spaces in the path, unlike -fsltx whose value is parsed with
  // sscanf("%[^ ] ") (truncated at the first space) from an ANSI command line
  // and then decoded as UTF-8 — both break non-ASCII/spaced paths.
  let mut run_params: Vec<String> = Vec::new();

  if config_guard.run_params.check_no_staging {
    run_params.push("-no_staging".to_string());
  }
  if config_guard.run_params.check_spawner {
    run_params.push("-dbg".to_string());
  }
  if config_guard.run_params.check_without_cache {
    run_params.push("-noprefetch".to_string());
  }
  if config_guard.run_params.checks {
    run_params.push("-checks".to_string());
  }
  if config_guard.run_params.ui_debug {
    run_params.push("-uidbg".to_string());
  }
  if config_guard.run_params.debug_spawn {
    run_params.push("-dbgsspwn".to_string());
  }
  let users_args = split_args(&config_guard.run_params.cmd_params);
  run_params.extend(users_args);

  // CLI args are for the xray engine directly (tiers 2 and 5). Stalker launcher
  // exes (tiers 1, 3, 4) are wrappers launched like a double-click: they set up
  // the engine themselves and read settings from fsgame.ltx/user.ltx. Passing
  // args to them can make the wrapper exit immediately without starting the game
  // (confirmed: 0.5.2-Beta's Stalker-CoC.exe exits with code 0 when given args).
  let launch_args: Vec<String> = if is_xray_engine {
    run_params
  } else {
    Vec::new()
  };

  log::info!(
    "Start game exe: {:?} with params: {:?} target_path: {:?}",
    &exe,
    &launch_args,
    target_path
  );

  // Direct xray-engine launches (tiers 2 and 5) resolve $fs_root$ from the CWD
  // via the ANSI Win32 API and decode it as UTF-8, so a non-ASCII CWD corrupts
  // it. Hide a non-ASCII CWD behind a virtual drive (subst) so the engine only
  // sees ASCII. Stalker-CoC.exe (tiers 3/4) is launched directly without subst:
  // it sets up the engine itself and handles non-ASCII paths.
  #[cfg(target_os = "windows")]
  let (effective_cwd, subst_drive): (PathBuf, Option<char>) = {
    let cwd_str = cwd.to_string_lossy();
    if is_xray_engine && !cwd_str.is_ascii() {
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

  // When a subst drive is active, launch the exe THROUGH it (e.g. Z:\Stalker-CoC.exe)
  // so that the launched process and any child it spawns (Stalker-CoC.exe ->
  // xrEngine.exe) see only ASCII paths via GetModuleFileName and inherited CWD.
  let launch_exe = match subst_drive {
    Some(drive) => exe
      .strip_prefix(&cwd)
      .map(|rel| PathBuf::from(format!("{}:\\", drive)).join(rel))
      .unwrap_or_else(|_| exe.clone()),
    None => exe.clone(),
  };

  log::info!(
    "run_game exe: {:?}, CWD: {:?} (is_xray_engine: {}, subst: {})",
    &launch_exe,
    &effective_cwd,
    is_xray_engine,
    subst_drive.is_some()
  );

  let child = Command::new(&launch_exe)
    .args(&launch_args)
    .current_dir(&effective_cwd)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|e| e.to_string())?;

  let wrapper_pid = child.id();
  // Keep handle only when we need to wait (subst cleanup); otherwise drop so
  // the process is fully detached from the launcher.
  #[cfg(target_os = "windows")]
  let child_for_subst = if subst_drive.is_some() { Some(child) } else { None };
  #[cfg(not(target_os = "windows"))]
  drop(child);

  let installed_for_pid = installed_path.clone();
  let cwd_for_pid = effective_cwd.clone();
  let engine_pid = tokio::task::spawn_blocking(move || {
    resolve_engine_pid(wrapper_pid, is_xray_engine, &cwd_for_pid, &installed_for_pid)
  })
  .await
  .map_err(|e| e.to_string())?;

  config_guard.latest_pid = i64::from(engine_pid);
  config_guard.save().map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  // If we created a subst drive, unmount it after the engine exits.
  // Wait on engine PID (wrapper may already be gone). Do not rewrite user.ltx.
  #[cfg(target_os = "windows")]
  {
    if let Some(drive) = subst_drive {
      let wait_pid = engine_pid;
      let _child = child_for_subst;
      tokio::task::spawn_blocking(move || {
        drop(_child);
        let mut system = System::new();
        let pid_sys = Pid::from(wait_pid as usize);
        loop {
          system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&[pid_sys]),
            true,
            ProcessRefreshKind::nothing(),
          );
          if !system.processes().contains_key(&pid_sys) {
            break;
          }
          std::thread::sleep(Duration::from_millis(500));
        }
        log::info!("Engine exited (pid {}); removing subst {}:", wait_pid, drive);
        subst_workaround::remove(drive);
      });
    }
  }

  Ok(engine_pid)
}

#[tauri::command]
pub fn get_passed_args() -> Vec<String> {
  let args: Vec<String> = std::env::args().skip(1).collect();
  log::info!("Passed args: {:?}", args);
  args
}

#[tauri::command]
pub fn is_process_alive(pid: u32) -> bool {
  let mut system = System::new();
  let pid_sys = Pid::from(pid as usize);
  system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid_sys]), true, ProcessRefreshKind::nothing());
  system.processes().contains_key(&pid_sys)
}

#[tauri::command]
pub fn open_explorer(path: String) -> Result<(), String> {
  let p = Path::new(&path);
  if !p.exists() {
    return Err(format!("Path does not exist: {}", path));
  }

  #[cfg(target_os = "windows")]
  {
    Command::new("explorer")
      .arg(path)
      .spawn()
      .map_err(|e| e.to_string())?;
  }

  #[cfg(target_os = "macos")]
  {
    Command::new("open").arg(path).spawn().map_err(|e| e.to_string())?;
  }

  #[cfg(target_os = "linux")]
  {
    Command::new("xdg-open")
      .arg(path)
      .spawn()
      .map_err(|e| e.to_string())?;
  }

  Ok(())
}
