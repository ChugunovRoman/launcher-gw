use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
use tokio::sync::Mutex;

use crate::configs::AppConfig::AppConfig;
use crate::configs::GameConfig::GameConfig;
use crate::configs::{RunParams, TmpLtx, UserLtx};
use crate::utils::split_args::split_args;
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
pub async fn run_game(
  app: tauri::AppHandle,
  user_ltx: tauri::State<'_, Arc<Mutex<UserLtx>>>,
  tmp_ltx: tauri::State<'_, Arc<Mutex<TmpLtx>>>,
) -> Result<u32, String> {
  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let mut config_guard = state.lock().await;

  let bin_path = Path::new(&config_guard.install_path).join("bin").join("xrEngine.exe");

  log::info!("run_game bin_path: {:?}", &bin_path);

  let mut user_config = user_ltx.lock().await;
  update_ltx_config(&mut user_config.0, &config_guard.run_params);

  let mut tmp_config = tmp_ltx.lock().await;
  update_ltx_config(&mut tmp_config.0, &config_guard.run_params);

  let fsgame_path = Path::new(&config_guard.install_path).join("fsgame.ltx");
  let mut run_params = vec![
    String::from("-fsltx"),
    fsgame_path.into_os_string().into_string().expect("Path to fsgame.ltx is not valid UTF-8"),
  ];

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

  log::info!("Start game bin_path: {:?} with params: {:?}", &bin_path, run_params);

  let child = Command::new(&bin_path)
    .args(run_params)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .spawn()
    .map_err(|e| e.to_string())?;

  config_guard.latest_pid = i64::from(child.id().clone());
  config_guard.save();

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
  system.refresh_processes_specifics(ProcessesToUpdate::Some(&[pid_sys]), true, ProcessRefreshKind::nothing());
  system.processes().contains_key(&pid_sys)
}

fn update_ltx_config(ltx: &mut GameConfig, run_params: &RunParams) {
  ltx.load().ok();

  ltx.set("vid_mode".to_string(), run_params.vid_mode.clone());

  ltx.set(
    "keypress_on_start".to_string(),
    if run_params.check_wait_press_any_key { "1" } else { "0" }.to_string(),
  );

  ltx.set("rs_v_sync".to_string(), if run_params.check_vsync { "1" } else { "0" }.to_string());

  ltx.set("rs_fullscreen".to_string(), if run_params.windowed_mode { "0" } else { "1" }.to_string());

  ltx.save().ok();
}
