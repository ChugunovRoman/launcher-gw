use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

use crate::configs::AppConfig::AppConfig;
use tauri::Manager;

#[tauri::command]
pub fn spawn_external_process(path: String, args: Vec<String>) -> Result<u32, String> {
  let child = Command::new(path)
    .args(args)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|e| e.to_string())?;

  Ok(child.id())
}

#[tauri::command]
pub fn run_game(app: tauri::AppHandle) -> Result<u32, String> {
  let state = app
    .try_state::<Arc<Mutex<AppConfig>>>()
    .ok_or("Config not initialized")?;
  let config_guard = state.lock().map_err(|_| "Poisoned mutex")?;

  let bin_path = Path::new(&config_guard.install_path)
    .join("bin")
    .join("xrEngine.exe");

  log::info!("run_game bin_path: {:?}", &bin_path);

  let fsgame_path = Path::new(&config_guard.install_path).join("fsgame.ltx");
  let mut run_params = vec![
    String::from("-fsltx"),
    fsgame_path
      .into_os_string()
      .into_string()
      .expect("Path to fsgame.ltx is not valid UTF-8"),
  ];

  run_params.extend(config_guard.run_params.cmd_params.clone());

  let child = Command::new(&bin_path)
    .args(run_params)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|e| e.to_string())?;

  Ok(child.id())
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
  system.refresh_processes_specifics(
    ProcessesToUpdate::Some(&[pid_sys]),
    true,
    ProcessRefreshKind::nothing(),
  );
  system.processes().contains_key(&pid_sys)
}
