use std::{
  path::{Path, PathBuf},
  sync::Arc,
};
use tauri::Manager;
use tokio::sync::Mutex;

use crate::{
  configs::{AppConfig::AppConfig, AppConfig::Version, GameConfig::GameConfig, RunParams, TmpLtx, UserLtx},
  consts::*,
  service::{get_release::ServiceGetRelease, keybind_manager::KeybindManager, main::Service},
  utils::resources::game_exe,
};

#[tauri::command]
pub async fn userltx_set_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;

  let releases = service_guard.get_local_version().await.map_err(|e| e.to_string())?;

  let installed_path = releases
    .iter()
    .find(|r| r.path == path)
    .map(|r| r.installed_path.clone())
    .ok_or_else(|| format!("Local version not found ! By path: {}", path))?;

  let state_userltx = app.try_state::<Arc<Mutex<UserLtx>>>().ok_or("UserLtx config not initialized")?;
  let mut userltx_guard = state_userltx.lock().await;

  let state_tmpltx = app.try_state::<Arc<Mutex<TmpLtx>>>().ok_or("TmpLtx config not initialized")?;
  let mut tmpltx_guard = state_tmpltx.lock().await;

  let version_path = Path::new(&installed_path);

  userltx_guard.0.set_file_path(&version_path.join(APPDATA_DIR).join(USER_LTX));
  tmpltx_guard.0.set_file_path(version_path.join(APPDATA_DIR).join(TMP_LTX));

  Ok(())
}

/// Resolve user.ltx / tmp.ltx for the version that Launch would prefer.
pub fn resolve_ltx_paths(config: &AppConfig) -> Option<(PathBuf, PathBuf)> {
  let version = resolve_active_version(config)?;
  let installed = PathBuf::from(&version.installed_path);
  let user = match &version.userltx_path {
    Some(p) => PathBuf::from(p),
    None => installed.join(APPDATA_DIR).join(USER_LTX),
  };
  let tmp = installed.join(APPDATA_DIR).join(TMP_LTX);
  Some((user, tmp))
}

fn resolve_active_version(config: &AppConfig) -> Option<Version> {
  // Prefer main game next to launcher (same priority as LaunchBtn mainVersion).
  let install = Path::new(&config.install_path);
  if install.join(BIN_DIR).join(game_exe()).exists() {
    return Some(Version {
      id: 0,
      name: String::new(),
      path: String::new(),
      installed_path: config.install_path.clone(),
      engine_path: None,
      fsgame_path: None,
      userltx_path: None,
      exe_path: None,
      download_path: String::new(),
      installed_updates: vec![],
      is_local: true,
      manifest: None,
    });
  }

  if let Some(name) = &config.selected_version {
    if let Some(v) = config.installed_versions.get(name) {
      return Some(v.clone());
    }
    if let Some(v) = config.versions.iter().find(|v| &v.name == name) {
      return Some(v.clone());
    }
  }

  if config.installed_versions.len() == 1 {
    return config.installed_versions.values().next().cloned();
  }

  None
}

/// Patch launcher-managed run_params cvars into an ltx file (preserves other keys).
pub fn apply_run_params_to_ltx(ltx_path: &Path, run_params: &RunParams) -> Result<(), String> {
  if let Some(parent) = ltx_path.parent() {
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
  }

  let mut ltx = GameConfig::new(ltx_path);
  ltx.load().map_err(|e| e.to_string())?;

  ltx.set("vid_mode".to_string(), run_params.vid_mode.clone());
  ltx.set("renderer".to_string(), render_to_ltx(run_params.render.clone()));
  let lang = lang_to_ltx(run_params.lang.clone());
  ltx.set("g_language".to_string(), lang.clone());
  ltx.set("g_language_ltx".to_string(), lang);
  ltx.set("fov".to_string(), run_params.fov.to_string());
  ltx.set("hud_fov".to_string(), run_params.hud_fov.to_string());
  ltx.set(
    "keypress_on_start".to_string(),
    if run_params.check_wait_press_any_key { "1" } else { "0" }.to_string(),
  );
  ltx.set(
    "rs_v_sync".to_string(),
    if run_params.check_vsync { "1" } else { "0" }.to_string(),
  );
  ltx.set(
    "rs_fullscreen".to_string(),
    if run_params.windowed_mode { "0" } else { "1" }.to_string(),
  );

  ltx.save().map_err(|e| e.to_string())
}

pub fn apply_run_params_to_version_ltx(config: &AppConfig) -> Result<(), String> {
  let Some((user_path, tmp_path)) = resolve_ltx_paths(config) else {
    log::warn!("apply_run_params_to_version_ltx: no active version path; skip user.ltx");
    return Ok(());
  };

  apply_run_params_to_ltx(&user_path, &config.run_params)?;
  apply_run_params_to_ltx(&tmp_path, &config.run_params)?;
  log::info!("Patched run_params into {:?} and {:?}", user_path, tmp_path);
  Ok(())
}

/// Pre-launch: patch run_params (+ optional keybind profile) into the target version's ltx files.
/// Does not touch files after the game exits.
pub async fn prepare_ltx_for_launch(
  user_ltx_path: &Path,
  tmp_ltx_path: &Path,
  run_params: &RunParams,
  keybind_manager: &KeybindManager,
  selected_profile: Option<&str>,
) -> Result<(), String> {
  apply_run_params_to_ltx(user_ltx_path, run_params)?;
  apply_run_params_to_ltx(tmp_ltx_path, run_params)?;

  if let Some(profile_name) = selected_profile {
    let profiles = keybind_manager.get_profiles().await;
    if let Some(profile_config) = profiles.get(profile_name) {
      let mut target = GameConfig::new(user_ltx_path);
      target
        .load()
        .map_err(|e| format!("Ошибка загрузки {}: {}", user_ltx_path.display(), e))?;
      target.merge(profile_config);
      target
        .save()
        .map_err(|e| format!("Ошибка сохранения {}: {}", user_ltx_path.display(), e))?;
      log::debug!("prepare_ltx_for_launch: merged profile '{}'", profile_name);
    } else {
      log::warn!("prepare_ltx_for_launch: profile '{}' not found", profile_name);
    }
  }

  Ok(())
}

pub async fn apply_selected_profile_to_version_ltx(
  config: &AppConfig,
  keybind_manager: &KeybindManager,
  profile_name: &str,
) -> Result<(), String> {
  let Some((user_path, _)) = resolve_ltx_paths(config) else {
    log::warn!("apply_selected_profile_to_version_ltx: no active version path; skip");
    return Ok(());
  };

  let profiles = keybind_manager.get_profiles().await;
  let profile_config = profiles
    .get(profile_name)
    .ok_or_else(|| format!("Профиль с именем '{}' не найден", profile_name))?;

  if let Some(parent) = user_path.parent() {
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
  }

  let mut target = GameConfig::new(&user_path);
  target.load().map_err(|e| format!("Ошибка загрузки {}: {}", user_path.display(), e))?;
  target.merge(profile_config);
  target
    .save()
    .map_err(|e| format!("Ошибка сохранения {}: {}", user_path.display(), e))?;

  log::info!("Applied profile '{}' to {:?}", profile_name, user_path);
  Ok(())
}

fn lang_to_ltx(lng: crate::configs::AppConfig::LangType) -> String {
  match lng {
    crate::configs::AppConfig::LangType::Rus => "rus".to_string(),
    crate::configs::AppConfig::LangType::Eng => "eng".to_string(),
  }
}

fn render_to_ltx(renderer: crate::configs::AppConfig::RenderType) -> String {
  match renderer {
    crate::configs::AppConfig::RenderType::RendererR2 => "renderer_r2".to_string(),
    crate::configs::AppConfig::RenderType::RendererR25 => "renderer_r2.5".to_string(),
    crate::configs::AppConfig::RenderType::RendererR3 => "renderer_r3".to_string(),
    crate::configs::AppConfig::RenderType::RendererR4 => "renderer_r4".to_string(),
    crate::configs::AppConfig::RenderType::RendererRgl => "renderer_rgl".to_string(),
  }
}
