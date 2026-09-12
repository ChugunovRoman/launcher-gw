use std::{
  path::{Path, PathBuf},
  sync::Arc,
};
use tauri::Manager;
use tokio::sync::Mutex;

use crate::{
  configs::{AlifeConfig::AlifeConfig, AppConfig::AppConfig, AppConfig::Version, GameConfig::GameConfig, RunParams, TmpLtx, UserLtx},
  consts::*,
  service::{index::IndexPreset, keybind_manager::KeybindManager},
  utils::resources::game_exe,
};

#[tauri::command]
pub async fn userltx_set_path(app: tauri::AppHandle, path: String) -> Result<(), String> {
  // Reads only AppConfig — no Service lock, so this never waits behind the
  // background network task's long-held Service lock (provider ping, index fetch, ...).
  let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("Config not initialized")?;
  let config_guard = state.lock().await;

  let releases = crate::service::get_release::get_local_version_from_config(&config_guard)
    .await
    .map_err(|e| e.to_string())?;

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

/// Game root for the version: the directory holding fsgame.ltx when set
/// manually, otherwise the install directory. Matches the CWD picked by
/// `resolve_launch_target` (handlers/process.rs).
pub fn resolve_game_root(version: &Version) -> PathBuf {
  if let Some(fsgame) = version.fsgame_path.as_ref() {
    if let Some(parent) = Path::new(fsgame).parent().filter(|p| !p.as_os_str().is_empty()) {
      return parent.to_path_buf();
    }
  }
  PathBuf::from(&version.installed_path)
}

/// Path to gamedata/configs/alife.ltx of the active version.
pub fn resolve_alife_ltx_path(config: &AppConfig) -> Option<PathBuf> {
  let version = resolve_active_version(config)?;
  Some(alife_ltx_path_in(&resolve_game_root(&version)))
}

/// gamedata/configs/alife.ltx inside the given game root.
pub fn alife_ltx_path_in(game_root: &Path) -> PathBuf {
  game_root.join(GAMEDATA_DIR).join(CONFIGS_DIR).join(ALIFE_LTX)
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

  let candidate = config
    .selected_version
    .as_deref()
    .and_then(|name| find_version_by_name(&config.installed_versions, &config.versions, name))
    .or_else(|| {
      if config.installed_versions.len() == 1 {
        config.installed_versions.values().next()
      } else {
        None
      }
    });

  // Never hand back a version without an install path: every caller joins it
  // into a file path, and an empty base silently produces a *relative* one
  // (user.ltx landing in `appdata/` next to the launcher, alife.ltx skipped
  // as "not found") instead of touching the real game directory.
  candidate.filter(|v| !v.installed_path.is_empty()).cloned()
}

/// Look up a version by `selected_version`, which stores the version *name*.
///
/// `installed_versions` is keyed by `path` (spaces replaced with dashes), so a
/// name like "Global War Dev" never matches the key "Global-War-Dev" — hence
/// the search over values by both fields before the key lookup. Remote entries
/// from `versions` are accepted only when they carry an install path: the
/// release list stores `installed_path: ""` for versions that are not
/// installed locally.
///
/// Same lookup order as `resolve_version_for_launch` (handlers/process.rs).
fn find_version_by_name<'a>(
  installed_versions: &'a std::collections::HashMap<String, Version>,
  versions: &'a [Version],
  name: &str,
) -> Option<&'a Version> {
  installed_versions
    .values()
    .find(|v| v.name == name || v.path == name)
    .or_else(|| installed_versions.get(name))
    .or_else(|| versions.iter().find(|v| (v.name == name || v.path == name) && !v.installed_path.is_empty()))
}

async fn load_presets(provider_id: Option<&str>) -> Vec<IndexPreset> {
  let provider_id = provider_id.unwrap_or(GITHUB_PID);
  match crate::service::index::load_index(provider_id).await {
    Ok(index) => index.presets,
    Err(e) => {
      log::warn!("load_presets: cannot load index for '{}': {}", provider_id, e);
      Vec::new()
    }
  }
}

/// Find the selected preset in the index. `None` — applying is disabled,
/// no preset is selected, the index is unavailable or the preset id is unknown.
async fn load_selected_preset(run_params: &RunParams, provider_id: Option<&str>) -> Option<IndexPreset> {
  if !run_params.apply_preset_on_launch || run_params.selected_preset_id.is_empty() {
    return None;
  }

  let presets = load_presets(provider_id).await;
  presets.into_iter().find(|p| p.id == run_params.selected_preset_id)
}

async fn apply_selected_preset(ltx: &mut GameConfig, run_params: &RunParams, provider_id: Option<&str>) {
  let Some(preset) = load_selected_preset(run_params, provider_id).await else {
    return;
  };

  for (key, value) in &preset.options {
    ltx.set(key.clone(), value.clone());
  }
}

/// Patch launcher-managed run_params cvars into an ltx file (preserves other keys).
pub async fn apply_run_params_to_ltx(ltx_path: &Path, run_params: &RunParams, provider_id: Option<&str>) -> Result<(), String> {
  if let Some(parent) = ltx_path.parent() {
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
  }

  let mut ltx = GameConfig::new(ltx_path);
  ltx.load().map_err(|e| e.to_string())?;

  apply_selected_preset(&mut ltx, run_params, provider_id).await;

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
  ltx.set("rs_v_sync".to_string(), if run_params.check_vsync { "1" } else { "0" }.to_string());
  ltx.set("rs_fullscreen".to_string(), if run_params.windowed_mode { "0" } else { "1" }.to_string());
  ltx.set("g_god".to_string(), if run_params.god_mode { "on" } else { "off" }.to_string());
  ltx.set(
    "g_unlimitedammo".to_string(),
    if run_params.unlimited_ammo { "on" } else { "off" }.to_string(),
  );
  ltx.set("rs_fps".to_string(), if run_params.show_fps { "on" } else { "off" }.to_string());
  ltx.set("rs_ids".to_string(), if run_params.show_ids { "on" } else { "off" }.to_string());
  ltx.set("r_font_legacy".to_string(), if run_params.font_legacy { "1" } else { "0" }.to_string());
  ltx.set("g_3d_scopes".to_string(), scope_type_to_ltx(run_params.scope_type));

  ltx.save().map_err(|e| e.to_string())
}

pub async fn apply_run_params_to_version_ltx(config: &AppConfig) -> Result<(), String> {
  let Some((user_path, tmp_path)) = resolve_ltx_paths(config) else {
    log::warn!("apply_run_params_to_version_ltx: no active version path; skip user.ltx");
    return Ok(());
  };

  apply_run_params_to_ltx(&user_path, &config.run_params, config.selected_provider_id.as_deref()).await?;
  apply_run_params_to_ltx(&tmp_path, &config.run_params, config.selected_provider_id.as_deref()).await?;
  log::info!("Patched run_params into {:?} and {:?}", user_path, tmp_path);
  Ok(())
}

/// Write the selected preset's alife settings into gamedata/configs/alife.ltx.
///
/// The file is NOT created when missing: the engine reads some keys of the
/// [alife] section via `r_float`/`r_u32` with an assert, and a file holding
/// only the preset keys would crash the game at startup.
pub async fn apply_preset_to_alife_ltx(alife_path: &Path, run_params: &RunParams, provider_id: Option<&str>) -> Result<(), String> {
  let Some(preset) = load_selected_preset(run_params, provider_id).await else {
    return Ok(());
  };

  if preset.alife.is_empty() {
    return Ok(());
  }

  let Some(mut ltx) = AlifeConfig::load(alife_path).map_err(|e| e.to_string())? else {
    log::warn!("apply_preset_to_alife_ltx: файл не найден, пропуск: {:?}", alife_path);
    return Ok(());
  };

  let mut applied = 0usize;
  for (key, value) in &preset.alife {
    if ltx.set_in_section(ALIFE_SECTION, key, value) {
      applied += 1;
    } else {
      log::warn!("apply_preset_to_alife_ltx: нет секции [{}] в {:?}", ALIFE_SECTION, alife_path);
      return Ok(());
    }
  }

  ltx.save().map_err(|e| e.to_string())?;
  log::info!("Применён preset '{}': {} ключей alife в {:?}", preset.id, applied, alife_path);
  Ok(())
}

/// Same as above, but the path is resolved from the active version in the config.
pub async fn apply_preset_to_version_alife_ltx(config: &AppConfig) -> Result<(), String> {
  let Some(alife_path) = resolve_alife_ltx_path(config) else {
    log::warn!("apply_preset_to_version_alife_ltx: активная версия не определена; пропуск alife.ltx");
    return Ok(());
  };

  apply_preset_to_alife_ltx(&alife_path, &config.run_params, config.selected_provider_id.as_deref()).await
}

/// Pre-launch: patch run_params (+ optional keybind profile) into the target version's ltx files.
/// Does not touch files after the game exits.
pub async fn prepare_ltx_for_launch(
  user_ltx_path: &Path,
  tmp_ltx_path: &Path,
  alife_ltx_path: &Path,
  run_params: &RunParams,
  provider_id: Option<&str>,
  keybind_manager: &KeybindManager,
  selected_profile: Option<&str>,
) -> Result<(), String> {
  apply_run_params_to_ltx(user_ltx_path, run_params, provider_id).await?;
  apply_run_params_to_ltx(tmp_ltx_path, run_params, provider_id).await?;

  // alife.ltx errors must not block the game launch.
  if let Err(e) = apply_preset_to_alife_ltx(alife_ltx_path, run_params, provider_id).await {
    log::warn!("prepare_ltx_for_launch: не удалось записать alife.ltx: {}", e);
  }

  if let Some(profile_name) = selected_profile {
    let profiles = keybind_manager.get_profiles().await;
    if let Some(profile_config) = profiles.get(profile_name) {
      let mut target = GameConfig::new(user_ltx_path);
      target.load().map_err(|e| format!("Ошибка загрузки {}: {}", user_ltx_path.display(), e))?;
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

pub async fn apply_selected_profile_to_version_ltx(config: &AppConfig, keybind_manager: &KeybindManager, profile_name: &str) -> Result<(), String> {
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
  target.save().map_err(|e| format!("Ошибка сохранения {}: {}", user_path.display(), e))?;

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

fn scope_type_to_ltx(scope_type: crate::configs::AppConfig::ScopeType) -> String {
  match scope_type {
    crate::configs::AppConfig::ScopeType::Scopes2dStatic => "0".to_string(),
    crate::configs::AppConfig::ScopeType::Scopes3d => "1".to_string(),
    crate::configs::AppConfig::ScopeType::Scopes2dRenderTarget => "2".to_string(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::collections::HashMap;

  fn version(name: &str, path: &str, installed_path: &str) -> Version {
    Version {
      id: 0,
      name: name.to_string(),
      path: path.to_string(),
      installed_path: installed_path.to_string(),
      engine_path: None,
      fsgame_path: None,
      userltx_path: None,
      exe_path: None,
      download_path: String::new(),
      installed_updates: vec![],
      is_local: true,
      manifest: None,
    }
  }

  const INSTALL_DIR: &str = r"D:\Games\GlobalWar\versions\Global-War-Dev";

  #[test]
  fn finds_installed_version_by_name_when_map_is_keyed_by_path() {
    // `selected_version` stores "Global War Dev", the map key is "Global-War-Dev".
    let mut installed = HashMap::new();
    installed.insert(
      "Global-War-Dev".to_string(),
      version("Global War Dev", "Global-War-Dev", INSTALL_DIR),
    );
    // The remote release list holds the same name with an empty install path.
    let remote = vec![version("Global War Dev", "Global-War-Dev", "")];

    let by_name = find_version_by_name(&installed, &remote, "Global War Dev").expect("lookup by name");
    assert_eq!(by_name.installed_path, INSTALL_DIR);

    let by_path = find_version_by_name(&installed, &remote, "Global-War-Dev").expect("lookup by path");
    assert_eq!(by_path.installed_path, INSTALL_DIR);
  }

  #[test]
  fn skips_remote_entries_without_install_path() {
    // Not installed locally: joining "" would yield a relative ltx path.
    let remote = vec![version("Global War Dev", "Global-War-Dev", "")];

    assert!(find_version_by_name(&HashMap::new(), &remote, "Global War Dev").is_none());
  }
}
