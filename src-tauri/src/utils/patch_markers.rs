use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Record of an installed patch. Persisted as a JSON marker file
/// `<install_path>/appdata/patches/<name>.json` so that patch state
/// lives alongside the game files (survives config loss, folder move,
/// and allows full releases to ship with patches pre-applied).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPatch {
  /// Patch tag name (matches the release tag in the updates repo).
  #[serde(default)]
  pub name: String,
  /// Provider id that was used to install this patch.
  #[serde(default)]
  pub provider_id: String,
  /// ISO-8601 timestamp when the patch was installed.
  #[serde(default)]
  pub installed_at: Option<String>,
  /// Release notes from the updates repo.
  #[serde(default)]
  pub notes: Option<String>,
  /// Faction-editor props this patch changes, for the UI (the authoritative
  /// data is the fragment file itself — see `service::faction_patch`).
  /// Empty — the patch carries no faction-editor settings.
  #[serde(default)]
  pub fe_fields: Vec<String>,
  /// File name of the fragment inside `appdata/patches`, when present.
  #[serde(default)]
  pub fe_fragment: Option<String>,
  /// ISO-8601 timestamp of the last time the player applied those settings.
  #[serde(default)]
  pub fe_applied_at: Option<String>,
}

/// Returns `<install_path>/appdata/patches`. Also holds the faction-editor
/// fragments patches carry (`service::faction_patch`), which is why it is
/// public.
pub fn patches_dir(install_path: &Path) -> PathBuf {
  install_path.join(crate::consts::APPDATA_DIR).join(crate::consts::PATCHES_DIR_NAME)
}

/// Reads all `*.json` marker files from the patches directory, parses
/// each as `InstalledPatch`, and returns them sorted by (installed_at, name).
/// Corrupted or unreadable files are skipped with a warning.
/// Missing directory → empty vec.
pub fn read_installed_patches(install_path: &Path) -> Vec<InstalledPatch> {
  let dir = patches_dir(install_path);
  if !dir.is_dir() {
    return Vec::new();
  }

  let mut patches: Vec<InstalledPatch> = Vec::new();

  let entries = match std::fs::read_dir(&dir) {
    Ok(e) => e,
    Err(e) => {
      log::warn!("patch_markers: cannot read {:?}: {}", dir, e);
      return Vec::new();
    }
  };

  for entry in entries.flatten() {
    let path = entry.path();
    if !path.is_file() {
      continue;
    }
    match path.extension().and_then(|e| e.to_str()) {
      Some("json") => {}
      _ => continue,
    }

    match std::fs::read_to_string(&path) {
      Ok(content) => match serde_json::from_str::<InstalledPatch>(&content) {
        Ok(p) => patches.push(p),
        Err(e) => {
          log::warn!("patch_markers: cannot parse {:?}: {}", path, e);
        }
      },
      Err(e) => {
        log::warn!("patch_markers: cannot read {:?}: {}", path, e);
      }
    }
  }

  // Sort by (installed_at, name) for deterministic ordering.
  patches.sort_by(|a, b| {
    let ta = a.installed_at.as_deref().unwrap_or("");
    let tb = b.installed_at.as_deref().unwrap_or("");
    ta.cmp(tb).then_with(|| a.name.cmp(&b.name))
  });

  patches
}

/// Rejects a patch name that could escape the patches directory.
///
/// Every path built from a patch name — the marker file, the faction-editor
/// fragment — goes through this. The name arrives over IPC from the frontend
/// and from downloaded release metadata, so it is never trusted input.
pub fn assert_safe_patch_name(name: &str) -> Result<()> {
  if name.is_empty() {
    anyhow::bail!("empty patch name");
  }
  // Guard against path traversal via malicious tag names.
  if name.contains('/') || name.contains('\\') || name.contains("..") || name.contains('\0') {
    anyhow::bail!("invalid patch name (contains path separator or '..'): {}", name);
  }
  // Catches what the character checks above cannot spell out on their own:
  // drive-relative names like `C:evil` are a single string but not a single
  // path component.
  if std::path::Path::new(name).components().count() != 1 {
    anyhow::bail!("invalid patch name (not a single path component): {}", name);
  }
  Ok(())
}

/// Path of one patch's marker file.
/// Rejects names that contain path separators or `..` (traversal guard).
pub fn marker_path(install_path: &Path, name: &str) -> Result<PathBuf> {
  assert_safe_patch_name(name)?;
  Ok(patches_dir(install_path).join(format!("{}.json", name)))
}

/// Writes a JSON marker file for the given patch.
/// Rejects names that contain path separators or `..` (traversal guard).
pub fn write_patch_marker(install_path: &Path, patch: &InstalledPatch) -> Result<()> {
  let file_path = marker_path(install_path, &patch.name)?;

  let dir = patches_dir(install_path);
  std::fs::create_dir_all(&dir).context("create patches marker dir")?;

  let json = serde_json::to_string_pretty(patch).context("serialize InstalledPatch")?;
  std::fs::write(&file_path, json).context("write patch marker file")?;

  log::info!("patch_markers: wrote {:?}", file_path);
  Ok(())
}

/// Reads one patch's marker. `Ok(None)` — the patch is not installed.
pub fn read_patch_marker(install_path: &Path, name: &str) -> Result<Option<InstalledPatch>> {
  let file_path = marker_path(install_path, name)?;
  if !file_path.is_file() {
    return Ok(None);
  }
  let content = std::fs::read_to_string(&file_path).with_context(|| format!("read {:?}", file_path))?;
  let patch = serde_json::from_str::<InstalledPatch>(&content).with_context(|| format!("parse {:?}", file_path))?;
  Ok(Some(patch))
}

/// Records that the player applied this patch's faction-editor settings,
/// keeping every other field of the marker as it was. No marker (e.g. the
/// patch shipped pre-applied inside a full release) — nothing to update.
pub fn mark_fe_applied(install_path: &Path, name: &str) -> Result<()> {
  let Some(mut patch) = read_patch_marker(install_path, name)? else {
    return Ok(());
  };
  patch.fe_applied_at = Some(chrono::Local::now().to_rfc3339());
  write_patch_marker(install_path, &patch)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn safe_patch_name_rejects_anything_that_could_escape() {
    for good in ["0.5.5-patch1", "patch_2", "v1.0"] {
      assert!(assert_safe_patch_name(good).is_ok(), "{} must be accepted", good);
    }
    for bad in ["", "..", "../evil", "..\\evil", "a/b", "a\\b", "C:evil", "C:\\evil", "/abs"] {
      assert!(assert_safe_patch_name(bad).is_err(), "{:?} must be rejected", bad);
    }
  }

  #[test]
  fn marker_path_stays_inside_the_patches_dir() {
    let install = std::path::Path::new("C:\\games\\gw");
    let path = marker_path(install, "0.5.5-patch1").unwrap();
    assert_eq!(path, patches_dir(install).join("0.5.5-patch1.json"));
    assert!(marker_path(install, "../../evil").is_err());
  }
}
