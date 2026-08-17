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
}

/// Returns `<install_path>/appdata/patches`.
fn patches_dir(install_path: &Path) -> PathBuf {
  install_path.join("appdata").join("patches")
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

/// Writes a JSON marker file for the given patch.
/// Rejects names that contain path separators or `..` (traversal guard).
pub fn write_patch_marker(install_path: &Path, patch: &InstalledPatch) -> Result<()> {
  // Guard against path traversal via malicious tag names.
  if patch.name.contains('/') || patch.name.contains('\\') || patch.name.contains("..") {
    anyhow::bail!("invalid patch name (contains path separator or '..'): {}", patch.name);
  }

  let dir = patches_dir(install_path);
  std::fs::create_dir_all(&dir).context("create patches marker dir")?;

  let file_path = dir.join(format!("{}.json", patch.name));
  let json = serde_json::to_string_pretty(patch).context("serialize InstalledPatch")?;
  std::fs::write(&file_path, json).context("write patch marker file")?;

  log::info!("patch_markers: wrote {:?}", file_path);
  Ok(())
}
