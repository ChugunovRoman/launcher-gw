//! Named `.gwfe` profiles the player collects locally: "save current as",
//! "apply", export/import a single file, rename/describe, delete.
//!
//! Modeled on `service::keybind_manager::KeybindManager` (same storage
//! pattern: one file per item in an app-data directory, id = file stem), but
//! profiles are small and rarely opened, so there is no long-lived in-memory
//! copy of their content — `list()` re-reads `manifest.json` from each file.
//! See `plans/launcher/faction-editor-settings-bundle-plan.md` §5 (Этап 2).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use tauri::path::BaseDirectory;
use tauri::Manager;
use tokio::sync::Mutex;

use crate::consts::*;
use crate::service::faction_settings::{self, BundleInspectResult, BundleManifest, FactionApplyResult};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactionProfileItem {
  pub id: String,
  pub manifest: BundleManifest,
}

#[derive(Clone)]
pub struct FactionProfileManager {
  dir: PathBuf,
  backups_dir: PathBuf,
  /// Best-effort cache of the last manifest seen for each id, used only to
  /// avoid a disk read right after a write this session just made. `list()`
  /// always re-scans the directory, so a stale/missing cache entry is
  /// harmless.
  cache: Arc<Mutex<HashMap<String, BundleManifest>>>,
  /// Serializes every operation that writes into `gamedata/configs`
  /// (apply / import-and-apply / reset): two concurrent applies would
  /// interleave backup, writes, deletions and rollback of each other.
  apply_lock: Arc<Mutex<()>>,
}

impl FactionProfileManager {
  pub fn new(app_handle: &tauri::AppHandle) -> Self {
    let dir = Self::init_dir(app_handle).unwrap_or_else(|e| {
      log::error!("FactionProfileManager: cannot init profiles directory: {}", e);
      std::env::temp_dir().join("gw-launcher-faction-profiles")
    });
    let backups_dir = dir.join(FE_BACKUP_DIR);
    if let Err(e) = fs::create_dir_all(&backups_dir) {
      log::error!("FactionProfileManager: cannot create backups directory: {}", e);
    }

    Self {
      dir,
      backups_dir,
      cache: Arc::new(Mutex::new(HashMap::new())),
      apply_lock: Arc::new(Mutex::new(())),
    }
  }

  pub fn backups_dir(&self) -> &Path {
    &self.backups_dir
  }

  /// Hold the returned guard for the whole duration of any operation that
  /// writes into the game's `gamedata/configs` (see `apply_lock`).
  pub async fn lock_apply(&self) -> tokio::sync::MutexGuard<'_, ()> {
    self.apply_lock.lock().await
  }

  /// Re-inspect a stored profile against the local install (adds the
  /// unknown-factions warning), for the confirmation dialog before apply.
  pub async fn inspect(&self, id: &str, game_root: &Path) -> Result<BundleInspectResult> {
    let path = self.path_for(id);
    if !path.exists() {
      anyhow::bail!("{}", FE_ERR_PROFILE_NOT_FOUND);
    }
    faction_settings::inspect_bundle(&path, Some(game_root))
  }

  pub fn profiles_dir(&self) -> &Path {
    &self.dir
  }

  /// List every profile, sorted by display name. Unreadable files (corrupt
  /// zip, foreign `.gwfe`) are skipped with a log warning rather than failing
  /// the whole list.
  pub async fn list(&self) -> Vec<FactionProfileItem> {
    let mut out = Vec::new();
    let Ok(read_dir) = fs::read_dir(&self.dir) else {
      return out;
    };

    for entry in read_dir.flatten() {
      let path = entry.path();
      if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some(FE_BUNDLE_EXT) {
        continue;
      }
      let Some(id) = path.file_stem().and_then(|s| s.to_str()) else { continue };

      match faction_settings::inspect_bundle(&path, None) {
        Ok(BundleInspectResult { manifest, .. }) => {
          self.cache.lock().await.insert(id.to_string(), manifest.clone());
          out.push(FactionProfileItem { id: id.to_string(), manifest });
        }
        Err(e) => log::warn!("FactionProfileManager: skipping unreadable profile '{}': {}", id, e),
      }
    }

    out.sort_by(|a, b| a.manifest.name.to_lowercase().cmp(&b.manifest.name.to_lowercase()));
    out
  }

  fn path_for(&self, id: &str) -> PathBuf {
    self.dir.join(format!("{}.{}", id, FE_BUNDLE_EXT))
  }

  /// Snapshot the game's current faction editor state into a new named profile.
  pub async fn save_current(&self, game_root: &Path, name: &str, description: &str, author: &str) -> Result<FactionProfileItem> {
    let validated = validate_profile_id(name)?;
    let dest = self.path_for(&validated);
    if dest.exists() {
      anyhow::bail!("{}", FE_ERR_PROFILE_EXISTS);
    }
    faction_settings::export_bundle(game_root, name, description, author, &dest)?;
    let manifest = faction_settings::inspect_bundle(&dest, None)?.manifest;
    self.cache.lock().await.insert(validated.clone(), manifest.clone());
    Ok(FactionProfileItem { id: validated, manifest })
  }

  /// Apply a stored profile onto `game_root` (backs up the current state and
  /// rolls back on error — see `faction_settings::apply_bundle`).
  pub async fn apply(&self, id: &str, game_root: &Path) -> Result<FactionApplyResult> {
    let path = self.path_for(id);
    if !path.exists() {
      anyhow::bail!("{}", FE_ERR_PROFILE_NOT_FOUND);
    }
    faction_settings::apply_bundle(&path, game_root, &self.backups_dir)
  }

  pub async fn export(&self, id: &str, dest_path: &Path) -> Result<()> {
    let path = self.path_for(id);
    if !path.exists() {
      anyhow::bail!("{}", FE_ERR_PROFILE_NOT_FOUND);
    }
    let ext = dest_path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
    if ext != FE_BUNDLE_EXT {
      anyhow::bail!("{}", FE_ERR_EXPORT_NOT_GWFE);
    }
    crate::utils::paths::assert_creatable_directory(dest_path.parent().unwrap_or(dest_path)).map_err(|e| anyhow::anyhow!("{}", e))?;
    fs::copy(&path, dest_path).with_context(|| format!("copy {} -> {}", path.display(), dest_path.display()))?;
    Ok(())
  }

  /// Copy an external `.gwfe` into the profiles directory as a new profile.
  /// The file is validated (`inspect_bundle`) before it is accepted, so a
  /// broken or foreign zip never enters the profiles list.
  pub async fn import(&self, src_path: &Path) -> Result<FactionProfileItem> {
    let ext = src_path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
    if ext != FE_BUNDLE_EXT {
      anyhow::bail!("{}", FE_ERR_EXPORT_NOT_GWFE);
    }

    let inspected = faction_settings::inspect_bundle(src_path, None)?;

    let base = src_path.file_stem().and_then(|s| s.to_str()).unwrap_or("profile");
    let base = validate_profile_id(base).unwrap_or_else(|_| "profile".to_string());
    let mut id = base.clone();
    let mut n = 1;
    while self.path_for(&id).exists() {
      n += 1;
      id = format!("{}_{}", base, n);
    }

    let dest = self.path_for(&id);
    fs::copy(src_path, &dest).with_context(|| format!("copy {} -> {}", src_path.display(), dest.display()))?;
    self.cache.lock().await.insert(id.clone(), inspected.manifest.clone());
    Ok(FactionProfileItem { id, manifest: inspected.manifest })
  }

  /// Rename/describe a profile in place. The id (file name) never changes —
  /// only `manifest.json`'s `name`/`description` — so an exported copy keeps
  /// working and the frontend's selection by id survives the edit.
  pub async fn update_meta(&self, id: &str, name: &str, description: &str) -> Result<FactionProfileItem> {
    let path = self.path_for(id);
    if !path.exists() {
      anyhow::bail!("{}", FE_ERR_PROFILE_NOT_FOUND);
    }
    let author = faction_settings::inspect_bundle(&path, None)?.manifest.author;
    faction_settings::update_bundle_meta(&path, name, description, &author)?;
    let manifest = faction_settings::inspect_bundle(&path, None)?.manifest;
    self.cache.lock().await.insert(id.to_string(), manifest.clone());
    Ok(FactionProfileItem { id: id.to_string(), manifest })
  }

  pub async fn delete(&self, id: &str) -> Result<()> {
    let path = self.path_for(id);
    if !path.exists() {
      anyhow::bail!("{}", FE_ERR_PROFILE_NOT_FOUND);
    }
    fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
    self.cache.lock().await.remove(id);
    Ok(())
  }

  fn init_dir(app_handle: &tauri::AppHandle) -> Result<PathBuf> {
    let config_dir = app_handle
      .path()
      .resolve(BASE_DIR, BaseDirectory::AppConfig)
      .context("Failed to resolve config directory")?
      .parent()
      .context("Resolved config directory path has no parent")?
      .to_path_buf();

    fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

    let profiles_dir = config_dir.join(FE_PROFILES_DIR);
    fs::create_dir_all(&profiles_dir).context("Failed to create faction profiles directory")?;

    Ok(profiles_dir)
  }
}

/// Validate + slugify a profile display name into a filesystem-safe id.
/// Filesystem-unsafe characters and reserved Windows device names are
/// rejected outright (matches `KeybindManager::validate_profile_name`); the
/// id is then reused as-is as the file stem, so it must also stay a sane
/// file name across platforms.
fn validate_profile_id(name: &str) -> Result<String> {
  let trimmed = name.trim();
  if trimmed.is_empty() {
    anyhow::bail!("{}", FE_ERR_PROFILE_NAME_INVALID);
  }
  const FORBIDDEN: &[char] = &['\\', '/', ':', '*', '?', '"', '<', '>', '|'];
  if trimmed.chars().any(|c| FORBIDDEN.contains(&c)) {
    anyhow::bail!("{}", FE_ERR_PROFILE_NAME_INVALID);
  }
  if trimmed.ends_with('.') || trimmed.ends_with(' ') {
    anyhow::bail!("{}", FE_ERR_PROFILE_NAME_INVALID);
  }
  const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
  ];
  let stem = trimmed.split_once('.').map_or(trimmed, |(s, _)| s);
  if RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
    anyhow::bail!("{}", FE_ERR_PROFILE_NAME_INVALID);
  }
  Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn validate_profile_id_rejects_unsafe_names() {
    assert!(validate_profile_id("Жёсткий баланс v2").is_ok());
    assert!(validate_profile_id("").is_err());
    assert!(validate_profile_id("a/b").is_err());
    assert!(validate_profile_id("a:b").is_err());
    assert!(validate_profile_id("trailing.").is_err());
    assert!(validate_profile_id("CON").is_err());
    assert!(validate_profile_id("com1").is_err());
  }
}
