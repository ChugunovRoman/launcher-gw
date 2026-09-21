//! Tauri commands for the faction editor settings bundle feature.
//! See `plans/launcher/faction-editor-settings-bundle-plan.md`.
//!
//! All the actual file work lives in `service::faction_settings` (pure,
//! testable against a fixture directory) and `service::faction_profile_manager`
//! (named profile storage). This module only resolves the active game's
//! `gamedata` root, checks the "game running" guard, and maps errors to the
//! `FE_ERR_*` / `FE_WARN_*` codes the frontend matches on.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::configs::AppConfig::AppConfig;
use crate::consts::*;
use crate::service::faction_profile_manager::{FactionProfileItem, FactionProfileManager};
use crate::service::faction_patch::FePatchApplyResult;
use crate::service::faction_settings::{self, BundleInspectResult, FactionApplyResult};
use crate::service::game_tracker::GameTracker;

/// `(game_root, engine_exe_path)` for the version this screen works with.
///
/// `version_name` is the version picked in the "Faction editor" screen: unlike
/// the rest of the launcher, this screen is explicitly NOT tied to the version
/// selected for launching — the player may keep several installs and edit the
/// factions of one while playing another. When it is empty, fall back to the
/// launch-time resolution (game next to the launcher, then `selected_version`,
/// then the single installed one).
///
/// The exe is resolved with the same tiers as the real launch
/// (`process::resolve_launch_target`: `exe_path` is RELATIVE to the install
/// dir, then `engine_path`, then `bin/xrEngine.exe`), so the "game running"
/// scan looks for the binary the launcher would actually start.
async fn resolve_context(
  app_config: &tauri::State<'_, Arc<Mutex<AppConfig>>>,
  version_name: Option<&str>,
) -> Result<(PathBuf, PathBuf), String> {
  let (_install_path, game_root, exe_path) = resolve_context_full(app_config, version_name).await?;
  Ok((game_root, exe_path))
}

/// `(install_path, game_root, engine_exe_path)`.
///
/// `install_path` is the version's own directory; `game_root` is where
/// `gamedata` lives and can differ from it when the version declares an
/// `fsgame_path`. Patch markers and the fragments patches carry live under
/// `install_path/appdata/patches`, the configs under `game_root/gamedata`, so
/// callers touching both need them apart.
async fn resolve_context_full(
  app_config: &tauri::State<'_, Arc<Mutex<AppConfig>>>,
  version_name: Option<&str>,
) -> Result<(PathBuf, PathBuf, PathBuf), String> {
  let cfg = app_config.lock().await;

  let version = match version_name.map(str::trim).filter(|n| !n.is_empty()) {
    Some(name) => crate::handlers::user_ltx::find_version_by_name(&cfg.installed_versions, &cfg.versions, name)
      .filter(|v| !v.installed_path.is_empty())
      .cloned()
      .ok_or_else(|| FE_ERR_NO_VERSION.to_string())?,
    None => crate::handlers::user_ltx::resolve_active_version(&cfg).ok_or_else(|| FE_ERR_NO_VERSION.to_string())?,
  };
  drop(cfg);

  let install_path = PathBuf::from(&version.installed_path);
  let game_root = crate::handlers::user_ltx::resolve_game_root(&version);
  let (exe_path, _cwd) = crate::handlers::process::resolve_launch_target(&version, Path::new(&version.installed_path));
  Ok((install_path, game_root, exe_path))
}

/// `Err(FE_ERR_GAME_RUNNING)` when the game is running — either tracked by
/// this launcher, or launched directly (a process whose exe path matches).
/// The full process scan is blocking, so it runs off the async runtime like
/// the other sysinfo scans (`process.rs`, `game_tracker.rs`).
async fn ensure_game_not_running(game_tracker: &tauri::State<'_, Arc<GameTracker>>, exe_path: &Path) -> Result<(), String> {
  if game_tracker.status().await.running {
    return Err(FE_ERR_GAME_RUNNING.to_string());
  }
  let exe = exe_path.to_path_buf();
  let running = tokio::task::spawn_blocking(move || faction_settings::exe_process_running(&exe))
    .await
    .map_err(|e| e.to_string())?;
  if running {
    return Err(FE_ERR_GAME_RUNNING.to_string());
  }
  Ok(())
}

/// Persist the author the player typed (empty clears the remembered value).
async fn remember_author(app_config: &tauri::State<'_, Arc<Mutex<AppConfig>>>, author: &str) {
  let trimmed = author.trim();
  let next = if trimmed.is_empty() { None } else { Some(trimmed.to_string()) };
  let mut cfg = app_config.lock().await;
  if cfg.faction_bundle_author != next {
    cfg.faction_bundle_author = next;
    if let Err(e) = cfg.save() {
      log::warn!("fe: failed to persist faction_bundle_author: {}", e);
    }
  }
}

fn assert_gwfe_extension(path: &Path) -> Result<(), String> {
  let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
  if ext != FE_BUNDLE_EXT {
    return Err(FE_ERR_EXPORT_NOT_GWFE.to_string());
  }
  Ok(())
}

/// `true` when the active game process is running (tracked or standalone) —
/// used by the frontend to show the "close the game first" dialog before it
/// even opens the "replace settings?" confirmation.
#[tauri::command]
pub async fn fe_is_game_running(
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  game_tracker: tauri::State<'_, Arc<GameTracker>>,
  versionName: Option<String>,
) -> Result<bool, String> {
  let (_, exe_path) = resolve_context(&app_config, versionName.as_deref()).await?;
  Ok(ensure_game_not_running(&game_tracker, &exe_path).await.is_err())
}

#[tauri::command]
pub async fn fe_export_bundle(
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  destPath: String,
  name: String,
  description: String,
  author: String,
  versionName: Option<String>,
) -> Result<(), String> {
  log::debug!("fe_export_bundle, destPath: {}, name: {}", &destPath, &name);
  let dest = PathBuf::from(&destPath);
  assert_gwfe_extension(&dest)?;
  // Same guard as keybind profile export: no system/temp destinations via IPC.
  crate::utils::paths::assert_creatable_directory(dest.parent().unwrap_or(&dest))?;

  let (game_root, _) = resolve_context(&app_config, versionName.as_deref()).await?;
  faction_settings::export_bundle(&game_root, &name, &description, &author, &dest).map_err(|e| e.to_string())?;

  remember_author(&app_config, &author).await;
  Ok(())
}

#[tauri::command]
pub async fn fe_inspect_bundle(
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  path: String,
  versionName: Option<String>,
) -> Result<BundleInspectResult, String> {
  let (game_root, _) = resolve_context(&app_config, versionName.as_deref()).await?;
  faction_settings::inspect_bundle(Path::new(&path), Some(&game_root)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fe_import_bundle(
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  game_tracker: tauri::State<'_, Arc<GameTracker>>,
  profile_manager: tauri::State<'_, Arc<FactionProfileManager>>,
  path: String,
  versionName: Option<String>,
) -> Result<FactionApplyResult, String> {
  log::debug!("fe_import_bundle, path: {}", &path);
  let _guard = profile_manager.lock_apply().await;
  let (game_root, exe_path) = resolve_context(&app_config, versionName.as_deref()).await?;
  ensure_game_not_running(&game_tracker, &exe_path).await?;
  faction_settings::apply_bundle(Path::new(&path), &game_root, profile_manager.backups_dir()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fe_reset_to_default(
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  game_tracker: tauri::State<'_, Arc<GameTracker>>,
  profile_manager: tauri::State<'_, Arc<FactionProfileManager>>,
  versionName: Option<String>,
) -> Result<FactionApplyResult, String> {
  log::debug!("fe_reset_to_default");
  let _guard = profile_manager.lock_apply().await;
  let (game_root, exe_path) = resolve_context(&app_config, versionName.as_deref()).await?;
  ensure_game_not_running(&game_tracker, &exe_path).await?;
  faction_settings::apply_defaults(&game_root, profile_manager.backups_dir()).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Faction-editor settings carried by a game patch
// ---------------------------------------------------------------------------

/// What a patch would change in the player's faction-editor config.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FePatchInspect {
  pub patch_name: String,
  /// Distinct prop names, for the dialog's list. Never used to apply anything
  /// — the section each value belongs to lives in the fragment.
  pub fields: Vec<String>,
  /// How many `(section, key)` pairs the fragment carries.
  pub entries: u32,
  pub fragment_path: String,
  /// When the player last applied this patch's settings, if ever.
  pub applied_at: Option<String>,
  /// `false` — the editor was never saved on this install, so there is no
  /// config to patch and the shipped defaults already carry the new values.
  pub has_player_config: bool,
}

/// Error string for the frontend: it matches on a leading `FE_ERR_*` code
/// (`lib/factionSettings.ts::factionErrorCode`), so anything that reached us
/// without one — an I/O error, a failed backup — is filed under "apply
/// failed" with the full cause chain as the detail.
fn fe_err(e: anyhow::Error) -> String {
  let text = format!("{:#}", e);
  if text.starts_with("FE_ERR_") {
    text
  } else {
    format!("{}: {}", FE_ERR_APPLY_FAILED, text)
  }
}

/// Locate a patch's fragment on disk. `Ok(None)` — this patch carries no
/// faction-editor settings.
async fn resolve_patch_fragment(
  app_config: &tauri::State<'_, Arc<Mutex<AppConfig>>>,
  version_name: Option<&str>,
  patch_name: &str,
) -> Result<Option<(PathBuf, PathBuf, PathBuf, PathBuf)>, String> {
  // The patch name reaches us over IPC and becomes part of a file path.
  crate::utils::patch_markers::assert_safe_patch_name(patch_name).map_err(|e| e.to_string())?;

  let (install_path, game_root, exe_path) = resolve_context_full(app_config, version_name).await?;
  let fragment_path =
    crate::service::faction_patch::fragment_path(&crate::utils::patch_markers::patches_dir(&install_path), patch_name);
  if !fragment_path.is_file() {
    return Ok(None);
  }
  Ok(Some((install_path, game_root, exe_path, fragment_path)))
}

#[tauri::command]
pub async fn fe_patch_inspect(
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  versionName: Option<String>,
  patchName: String,
) -> Result<Option<FePatchInspect>, String> {
  let Some((install_path, game_root, _exe, fragment_path)) =
    resolve_patch_fragment(&app_config, versionName.as_deref(), &patchName).await?
  else {
    return Ok(None);
  };

  let fragment = match crate::service::faction_patch::read_fragment(&fragment_path).map_err(fe_err)? {
    Some(fragment) => fragment,
    None => return Ok(None),
  };

  // The marker only adds "when was this applied"; a corrupt one must not hide
  // a perfectly valid fragment behind an error.
  let applied_at = match crate::utils::patch_markers::read_patch_marker(&install_path, &patchName) {
    Ok(marker) => marker.and_then(|m| m.fe_applied_at),
    Err(e) => {
      log::warn!("fe_patch_inspect: cannot read marker of '{}': {}", patchName, e);
      None
    }
  };

  let has_player_config = game_root.join(GAMEDATA_DIR).join(CONFIGS_DIR).join(FE_CONFIG_LTX).is_file();

  Ok(Some(FePatchInspect {
    patch_name: patchName,
    entries: fragment.edits.len() as u32,
    fields: fragment.fields,
    fragment_path: fragment_path.to_string_lossy().into_owned(),
    applied_at,
    has_player_config,
  }))
}

#[tauri::command]
pub async fn fe_patch_apply(
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  game_tracker: tauri::State<'_, Arc<GameTracker>>,
  profile_manager: tauri::State<'_, Arc<FactionProfileManager>>,
  versionName: Option<String>,
  patchName: String,
  // Props the player left ticked in the confirmation dialog. `None` — apply
  // everything the fragment carries.
  fields: Option<Vec<String>>,
) -> Result<FePatchApplyResult, String> {
  log::debug!("fe_patch_apply, patch: {}, fields: {:?}", &patchName, &fields);
  // Same guard as import/reset: these all rewrite the very same config files.
  let _guard = profile_manager.lock_apply().await;

  let Some((install_path, game_root, exe_path, fragment_path)) =
    resolve_patch_fragment(&app_config, versionName.as_deref(), &patchName).await?
  else {
    return Err(FE_ERR_PATCH_NOT_FOUND.to_string());
  };
  ensure_game_not_running(&game_tracker, &exe_path).await?;

  let fragment = crate::service::faction_patch::read_fragment(&fragment_path)
    .map_err(fe_err)?
    .ok_or_else(|| FE_ERR_PATCH_NOT_FOUND.to_string())?;

  // Unticking a prop drops it in every section at once — the same filter the
  // developer's "collect patch" screen uses. The dialog disables its confirm
  // button when nothing is left, so this is the defensive half of that.
  let fragment = match fields {
    Some(fields) => fragment.retain_fields(&fields),
    None => fragment,
  };
  if fragment.is_empty() {
    return Err(FE_ERR_PATCH_EMPTY.to_string());
  }

  let result = crate::service::faction_patch::apply_fragment(&game_root, profile_manager.backups_dir(), &fragment).map_err(fe_err)?;

  // Only a real write counts as "applied" — a no-op (the player never opened
  // the editor) must not mark the patch as done, so the button still works
  // once they do.
  if result.outcome == faction_settings::ApplyOutcome::Applied && result.applied > 0 {
    if let Err(e) = crate::utils::patch_markers::mark_fe_applied(&install_path, &patchName) {
      // The settings are already on disk; failing to note that is not worth
      // reporting as a failed apply.
      log::warn!("fe_patch_apply: cannot update marker of '{}': {}", patchName, e);
    }
  }

  Ok(result)
}

// ---------------------------------------------------------------------------
// Profile manager
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn fe_profiles_list(profile_manager: tauri::State<'_, Arc<FactionProfileManager>>) -> Result<Vec<FactionProfileItem>, String> {
  Ok(profile_manager.list().await)
}

#[tauri::command]
pub async fn fe_profile_save_current(
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  profile_manager: tauri::State<'_, Arc<FactionProfileManager>>,
  name: String,
  description: String,
  author: String,
  versionName: Option<String>,
) -> Result<FactionProfileItem, String> {
  log::debug!("fe_profile_save_current, name: {}", &name);
  let (game_root, _) = resolve_context(&app_config, versionName.as_deref()).await?;
  let item = profile_manager
    .save_current(&game_root, &name, &description, &author)
    .await
    .map_err(|e| e.to_string())?;
  remember_author(&app_config, &author).await;
  Ok(item)
}

#[tauri::command]
pub async fn fe_profile_apply(
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  game_tracker: tauri::State<'_, Arc<GameTracker>>,
  profile_manager: tauri::State<'_, Arc<FactionProfileManager>>,
  id: String,
  versionName: Option<String>,
) -> Result<FactionApplyResult, String> {
  log::debug!("fe_profile_apply, id: {}", &id);
  let _guard = profile_manager.lock_apply().await;
  let (game_root, exe_path) = resolve_context(&app_config, versionName.as_deref()).await?;
  ensure_game_not_running(&game_tracker, &exe_path).await?;
  profile_manager.apply(&id, &game_root).await.map_err(|e| e.to_string())
}

/// Re-inspect a stored profile against the local install — feeds the
/// confirmation dialog (unknown-factions warning) before `fe_profile_apply`.
#[tauri::command]
pub async fn fe_profile_inspect(
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  profile_manager: tauri::State<'_, Arc<FactionProfileManager>>,
  id: String,
  versionName: Option<String>,
) -> Result<BundleInspectResult, String> {
  let (game_root, _) = resolve_context(&app_config, versionName.as_deref()).await?;
  profile_manager.inspect(&id, &game_root).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fe_profile_export(
  profile_manager: tauri::State<'_, Arc<FactionProfileManager>>,
  id: String,
  destPath: String,
) -> Result<(), String> {
  log::debug!("fe_profile_export, id: {}, destPath: {}", &id, &destPath);
  profile_manager.export(&id, Path::new(&destPath)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fe_profile_import(
  profile_manager: tauri::State<'_, Arc<FactionProfileManager>>,
  path: String,
) -> Result<FactionProfileItem, String> {
  log::debug!("fe_profile_import, path: {}", &path);
  profile_manager.import(Path::new(&path)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fe_profile_update_meta(
  profile_manager: tauri::State<'_, Arc<FactionProfileManager>>,
  id: String,
  name: String,
  description: String,
) -> Result<FactionProfileItem, String> {
  log::debug!("fe_profile_update_meta, id: {}, name: {}", &id, &name);
  profile_manager.update_meta(&id, &name, &description).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fe_profile_delete(profile_manager: tauri::State<'_, Arc<FactionProfileManager>>, id: String) -> Result<(), String> {
  log::debug!("fe_profile_delete, id: {}", &id);
  profile_manager.delete(&id).await.map_err(|e| e.to_string())
}

/// Remember which game version the "Faction editor" screen works with.
/// `None`/empty clears it, restoring the launch-time fallback.
#[tauri::command]
pub async fn fe_set_version(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>, versionName: Option<String>) -> Result<(), String> {
  let next = versionName.map(|n| n.trim().to_string()).filter(|n| !n.is_empty());
  log::debug!("fe_set_version, versionName: {:?}", &next);

  let mut cfg = app_config.lock().await;
  if cfg.faction_settings_version != next {
    cfg.faction_settings_version = next;
    cfg.save().map_err(|e| e.to_string())?;
  }
  Ok(())
}

/// Path of the profiles directory, for the frontend's "open folder" button
/// (opened via the existing `open_explorer` command).
#[tauri::command]
pub async fn fe_profiles_dir(profile_manager: tauri::State<'_, Arc<FactionProfileManager>>) -> Result<String, String> {
  Ok(profile_manager.profiles_dir().to_string_lossy().into_owned())
}
