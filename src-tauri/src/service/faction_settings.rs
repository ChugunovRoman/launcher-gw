//! Faction editor settings bundle (`.gwfe`): pack/inspect/apply the files the
//! in-game faction editor writes under `gamedata/configs`, so players can
//! export and share them, or reset to the mod's shipped defaults.
//!
//! Spec: `plans/launcher/faction-editor-settings-bundle-plan.md`.
//!
//! This module is pure (no Tauri types) and operates on any `game_root: &Path`
//! so it is fully unit-testable against a fixture directory — see the tests
//! module below. `handlers::faction_settings` wires it to Tauri commands.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::configs::atomic_write_bytes;
use crate::configs::AlifeConfig::AlifeConfig;
use crate::consts::*;
use crate::utils::hash::hex_lower;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// One file inside a `.gwfe` archive, already decoded from / ready to encode
/// into zip bytes.
#[derive(Debug, Clone)]
struct BundleFile {
  /// Path inside the archive, forward slashes (e.g. `configs/faction_editor_config.ltx`).
  archive_path: String,
  bytes: Vec<u8>,
  mode: BundleFileMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BundleFileMode {
  /// Written byte-for-byte to `target` (`configs/...`, relative to `gamedata`).
  Replace { target: String },
  /// Only the `key = value` pairs found in this fragment are patched into
  /// `section` of `target`; everything else in `target` stays untouched.
  MergeKeys { target: String, section: String },
}

/// The faction editor's current on-disk state, ready to be written as a bundle.
pub struct CollectedState {
  files: Vec<BundleFile>,
  pub warnings: Vec<String>,
  pub summary: BundleSummary,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BundleSummary {
  #[serde(default)]
  pub factions_total: u32,
  #[serde(default)]
  pub factions_created: u32,
  #[serde(default)]
  pub custom_armament: Vec<String>,
  #[serde(default)]
  pub custom_squad_sizes: Vec<String>,
  #[serde(default)]
  pub has_relations: bool,
  #[serde(default)]
  pub has_population: bool,
  #[serde(default)]
  pub has_point_types: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleManifestFile {
  pub path: String,
  pub mode: String,
  pub size: u64,
  pub sha256: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleManifest {
  #[serde(default)]
  pub schema: u32,
  #[serde(default)]
  pub kind: String,
  #[serde(default)]
  pub name: String,
  #[serde(default)]
  pub description: String,
  #[serde(default)]
  pub author: String,
  #[serde(default)]
  pub created_at: String,
  #[serde(default)]
  pub files: Vec<BundleManifestFile>,
  #[serde(default)]
  pub summary: BundleSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleInspectResult {
  pub manifest: BundleManifest,
  pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ApplyOutcome {
  Applied,
  Failed,
  RolledBack,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactionApplyResult {
  pub outcome: ApplyOutcome,
  pub warnings: Vec<String>,
  pub backup_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Export the current state under `game_root` into a new `.gwfe` at `dest_path`.
/// Fails with `FE_ERR_NO_CONFIG` if the editor was never saved.
pub fn export_bundle(game_root: &Path, name: &str, description: &str, author: &str, dest_path: &Path) -> Result<()> {
  let state = collect_state(game_root)?;
  write_bundle(&state, name, description, author, dest_path)
}

/// Read the faction editor's current on-disk state. `Err(FE_ERR_NO_CONFIG)` —
/// `faction_editor_config.ltx` does not exist (editor never saved).
pub fn collect_state(game_root: &Path) -> Result<CollectedState> {
  resolved_from_disk(game_root)?.ok_or_else(|| anyhow!("{}", FE_ERR_NO_CONFIG))
}

/// Open a `.gwfe` and read only `manifest.json`, validating it against the
/// whitelist and size limits. Nothing is written to disk and nothing but the
/// manifest entry (plus a name pass over the other entries) is read.
/// `game_root`, when given, adds a warning listing factions the bundle
/// carries that the local install's default config does not know.
pub fn inspect_bundle(bundle_path: &Path, game_root: Option<&Path>) -> Result<BundleInspectResult> {
  let file = fs::File::open(bundle_path).with_context(|| format!("open {}", bundle_path.display()))?;
  let bundle_len = file.metadata().map(|m| m.len()).unwrap_or(0);
  if bundle_len > FE_MAX_BUNDLE_SIZE {
    bail!("{}", FE_ERR_BUNDLE_TOO_LARGE);
  }

  let mut archive = ZipArchive::new(file).map_err(|_| anyhow!("{}", FE_ERR_BUNDLE_NOT_ZIP))?;

  let manifest: BundleManifest = {
    let entry = archive.by_name(MANIFEST_NAME).map_err(|_| anyhow!("{}", FE_ERR_BUNDLE_NO_MANIFEST))?;
    if entry.size() > FE_MAX_FILE_SIZE {
      bail!("{}", FE_ERR_BUNDLE_TOO_LARGE);
    }
    let buf = read_entry_bounded(entry, FE_MAX_FILE_SIZE).context("read manifest.json")?;
    serde_json::from_slice(&buf).map_err(|_| anyhow!("{}", FE_ERR_BUNDLE_NO_MANIFEST))?
  };

  if manifest.kind != FE_BUNDLE_KIND {
    bail!("{}", FE_ERR_BUNDLE_KIND);
  }
  if manifest.schema == 0 || manifest.schema > FE_BUNDLE_SCHEMA {
    bail!("{}", FE_ERR_BUNDLE_SCHEMA);
  }
  if manifest.name.trim().is_empty() {
    bail!("{}", FE_ERR_BUNDLE_NO_MANIFEST);
  }

  let mut total_declared: u64 = 0;
  let mut seen: HashSet<&str> = HashSet::new();
  for f in &manifest.files {
    // `path` decides everything: whitelist, mode and the write target. The
    // manifest's own `mode`/`target` fields are checked for consistency and
    // otherwise IGNORED downstream — a hand-crafted manifest must not be able
    // to redirect a whitelisted entry to another file (path traversal).
    let Some((expected_mode, _target)) = expected_mode_and_target(&f.path) else {
      bail!("{}: {}", FE_ERR_BUNDLE_UNKNOWN_FILE, f.path);
    };
    if f.mode != expected_mode {
      bail!("{}: {} ({})", FE_ERR_BUNDLE_UNKNOWN_FILE, f.path, f.mode);
    }
    if !seen.insert(f.path.as_str()) {
      bail!("{}: duplicate {}", FE_ERR_BUNDLE_UNKNOWN_FILE, f.path);
    }
    if f.size > FE_MAX_FILE_SIZE {
      bail!("{}", FE_ERR_BUNDLE_TOO_LARGE);
    }
    total_declared = total_declared.saturating_add(f.size);
  }
  if total_declared > FE_MAX_BUNDLE_SIZE {
    bail!("{}", FE_ERR_BUNDLE_TOO_LARGE);
  }
  if !seen.contains(FE_BUNDLE_CONFIG_PATH) {
    bail!("{}", FE_ERR_BUNDLE_INVALID_CONFIG);
  }

  // Every entry other than the manifest must be whitelisted, even if it is
  // not declared in `files` — a smuggled entry is refused even though it
  // would never be applied (defence in depth against a hand-crafted zip).
  let mut in_archive: HashSet<String> = HashSet::new();
  for i in 0..archive.len() {
    let entry = archive.by_index(i).context("read zip entry")?;
    if entry.is_dir() || entry.name() == MANIFEST_NAME {
      continue;
    }
    // Every data entry must be both whitelisted AND declared in `files[]`
    // (so its hash gets verified on apply) — no undeclared payload.
    if !is_whitelisted_archive_path(entry.name()) || !seen.contains(entry.name()) {
      bail!("{}: {}", FE_ERR_BUNDLE_UNKNOWN_FILE, entry.name());
    }
    in_archive.insert(entry.name().to_string());
  }
  // ...and every declared file must exist, so a truncated archive is refused
  // here (import into the profile list) rather than only on apply.
  for f in &manifest.files {
    if !in_archive.contains(&f.path) {
      bail!("{}: {}", FE_ERR_BUNDLE_NO_MANIFEST, f.path);
    }
  }

  let mut warnings = Vec::new();
  if let Some(game_root) = game_root {
    if let Ok(entry) = archive.by_name(FE_BUNDLE_CONFIG_PATH) {
      if let Ok(buf) = read_entry_bounded(entry, FE_MAX_FILE_SIZE) {
        let bundle_factions: HashSet<String> =
          faction_sections(&String::from_utf8_lossy(&buf)).into_iter().map(|(name, _)| name).collect();

        let default_path = game_root.join(GAMEDATA_DIR).join(CONFIGS_DIR).join(FE_DEFAULT_CONFIG_LTX);
        if let Ok(default_bytes) = fs::read(&default_path) {
          let local_factions: HashSet<String> =
            faction_sections(&String::from_utf8_lossy(&default_bytes)).into_iter().map(|(name, _)| name).collect();
          let mut unknown: Vec<String> = bundle_factions.difference(&local_factions).cloned().collect();
          if !unknown.is_empty() {
            unknown.sort();
            warnings.push(format!("{}: {}", FE_WARN_UNKNOWN_FACTIONS, unknown.join(", ")));
          }
        }
      }
    }
  }

  Ok(BundleInspectResult { manifest, warnings })
}

/// Import + apply a `.gwfe` onto `game_root`. Backs up the current state into
/// `backups_dir` first, and rolls back to it on any write error.
pub fn apply_bundle(bundle_path: &Path, game_root: &Path, backups_dir: &Path) -> Result<FactionApplyResult> {
  let (_manifest, files) = read_and_verify_bundle(bundle_path)?;
  apply_with_backup(game_root, backups_dir, &files)
}

/// Reset the managed file set to the mod's shipped defaults
/// (`faction_editor_default_config.ltx` + no custom files). Mirrors the
/// editor's in-game "reset all" (see the plan §4.6).
pub fn apply_defaults(game_root: &Path, backups_dir: &Path) -> Result<FactionApplyResult> {
  let files = default_files(game_root)?;
  apply_with_backup(game_root, backups_dir, &files)
}

/// Rewrite `manifest.json` (name/description/author) inside an existing
/// `.gwfe`, keeping every other entry byte-for-byte. Used by the profile
/// manager's rename/describe action, so re-sharing a renamed profile still
/// produces the same file content.
pub fn update_bundle_meta(bundle_path: &Path, name: &str, description: &str, author: &str) -> Result<()> {
  let name = name.trim();
  if name.is_empty() {
    bail!("bundle name must not be empty");
  }
  if name.chars().count() > FE_MAX_NAME_LEN {
    bail!("bundle name too long");
  }
  if description.chars().count() > FE_MAX_DESC_LEN {
    bail!("bundle description too long");
  }
  if author.chars().count() > FE_MAX_AUTHOR_LEN {
    bail!("bundle author too long");
  }

  let mut manifest = inspect_bundle(bundle_path, None)?.manifest;
  manifest.name = name.to_string();
  manifest.description = description.trim().to_string();
  manifest.author = author.trim().to_string();
  let manifest_json = serde_json::to_vec_pretty(&manifest).context("serialize manifest.json")?;

  let file = fs::File::open(bundle_path).with_context(|| format!("open {}", bundle_path.display()))?;
  let mut archive = ZipArchive::new(file).map_err(|_| anyhow!("{}", FE_ERR_BUNDLE_NOT_ZIP))?;

  let tmp_path = bundle_path.with_extension(format!("{}.{}.tmp", FE_BUNDLE_EXT, std::process::id()));
  {
    let out = fs::File::create(&tmp_path).with_context(|| format!("create {}", tmp_path.display()))?;
    let mut zip = ZipWriter::new(out);
    let options: FileOptions<'_, ()> = FileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file(MANIFEST_NAME, options).context("start manifest.json entry")?;
    zip.write_all(&manifest_json).context("write manifest.json entry")?;

    for i in 0..archive.len() {
      let entry = archive.by_index(i).context("read zip entry")?;
      if entry.is_dir() || entry.name() == MANIFEST_NAME {
        continue;
      }
      let entry_name = entry.name().to_string();
      // inspect_bundle above already refused anything not whitelisted, so
      // this is only the size bound (never trust `size()` for allocation).
      let bytes = read_entry_bounded(entry, FE_MAX_FILE_SIZE).with_context(|| format!("read {}", entry_name))?;
      zip.start_file(&entry_name, options).with_context(|| format!("start entry {}", entry_name))?;
      zip.write_all(&bytes).with_context(|| format!("write entry {}", entry_name))?;
    }
    zip.finish().context("finish zip")?;
  }
  fs::rename(&tmp_path, bundle_path).with_context(|| format!("rename {} -> {}", tmp_path.display(), bundle_path.display()))?;

  Ok(())
}

/// `true` if a process whose executable resolves to `exe_path` is currently
/// running. Independent of `GameTracker`: catches a direct double-click on
/// the engine binary, bypassing the launcher entirely.
pub fn exe_process_running(exe_path: &Path) -> bool {
  use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

  let mut system = System::new();
  system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_exe(UpdateKind::Always));

  // Cheap file-name pre-filter, then a canonical comparison: the configured
  // path may differ from the OS-reported one in case, separators, `..`
  // segments or the `\\?\` prefix.
  let target_name = exe_path.file_name().map(|n| n.to_string_lossy().to_lowercase());
  let target_canon = fs::canonicalize(exe_path).ok();
  let target_str = exe_path.to_string_lossy();

  system.processes().values().any(|p| {
    let Some(current) = p.exe() else { return false };
    let same_name = match (&target_name, current.file_name()) {
      (Some(t), Some(c)) => *t == c.to_string_lossy().to_lowercase(),
      _ => false,
    };
    if !same_name {
      return false;
    }
    match (&target_canon, fs::canonicalize(current).ok()) {
      (Some(t), Some(c)) => t == &c,
      _ => paths_equal(&target_str, current),
    }
  })
}

// ---------------------------------------------------------------------------
// Collecting the on-disk state
// ---------------------------------------------------------------------------

fn resolved_from_disk(game_root: &Path) -> Result<Option<CollectedState>> {
  let configs_dir = game_root.join(GAMEDATA_DIR).join(CONFIGS_DIR);
  let config_path = configs_dir.join(FE_CONFIG_LTX);
  if !config_path.exists() {
    return Ok(None);
  }
  let config_bytes = fs::read(&config_path).with_context(|| format!("read {}", config_path.display()))?;
  let config_text = String::from_utf8_lossy(&config_bytes).into_owned();

  let mut files = vec![BundleFile {
    archive_path: FE_BUNDLE_CONFIG_PATH.to_string(),
    bytes: config_bytes,
    mode: BundleFileMode::Replace { target: format!("configs/{}", FE_CONFIG_LTX) },
  }];

  let mut warnings = Vec::new();
  let mut has_relations = false;
  let mut has_population = false;
  let mut has_point_types = false;

  for (rel, archive_path) in [
    (FE_GAME_RELATIONS_CUSTOM, FE_BUNDLE_RELATIONS_PATH),
    (FE_DEFAULT_CUSTOM_SIM, FE_BUNDLE_DEFAULT_CUSTOM_PATH),
    (FE_SIM_OBJECTS_PROPS_CUSTOM, FE_BUNDLE_SIM_PROPS_CUSTOM_PATH),
  ] {
    let path = configs_dir.join(rel_to_path(rel));
    if path.exists() {
      let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
      match rel {
        r if r == FE_GAME_RELATIONS_CUSTOM => has_relations = true,
        r if r == FE_DEFAULT_CUSTOM_SIM => has_population = true,
        r if r == FE_SIM_OBJECTS_PROPS_CUSTOM => has_point_types = true,
        _ => {}
      }
      files.push(BundleFile {
        archive_path: archive_path.to_string(),
        bytes,
        mode: BundleFileMode::Replace { target: format!("configs/{}", rel) },
      });
    }
  }

  let custom_armament = read_custom_dir(&configs_dir.join(rel_to_path(FE_ARMAMENT_CUSTOM_DIR)))?;
  for (name, bytes) in &custom_armament {
    files.push(BundleFile {
      archive_path: format!("{}/{}.ltx", FE_BUNDLE_ARMAMENT_CUSTOM_DIR, name),
      bytes: bytes.clone(),
      mode: BundleFileMode::Replace { target: format!("configs/{}/{}.ltx", FE_ARMAMENT_CUSTOM_DIR, name) },
    });
  }

  let custom_squad_sizes = read_custom_dir(&configs_dir.join(rel_to_path(FE_SQUAD_DESCR_CUSTOM_DIR)))?;
  for (name, bytes) in &custom_squad_sizes {
    files.push(BundleFile {
      archive_path: format!("{}/{}.ltx", FE_BUNDLE_SQUAD_DESCR_CUSTOM_DIR, name),
      bytes: bytes.clone(),
      mode: BundleFileMode::Replace { target: format!("configs/{}/{}.ltx", FE_SQUAD_DESCR_CUSTOM_DIR, name) },
    });
  }

  // axr_options.partial.ltx: only keys that already exist in axr_options.ltx,
  // for the 4 Common flags plus every faction found in the live config.
  let axr_path = configs_dir.join(FE_AXR_OPTIONS_LTX);
  let mut axr_pairs = Vec::new();
  if let Some(axr) = AlifeConfig::load(&axr_path)? {
    let existing: HashMap<String, String> = axr.get_section(FE_AXR_OPTIONS_SECTION).into_iter().collect();
    for flag in FE_AXR_COMMON_FLAGS {
      if let Some(value) = existing.get(*flag) {
        axr_pairs.push(((*flag).to_string(), value.clone()));
      }
    }
    for (faction, _created) in faction_sections(&config_text) {
      for key in axr_faction_keys(&faction) {
        if let Some(value) = existing.get(&key) {
          axr_pairs.push((key, value.clone()));
        }
      }
    }
  } else {
    warnings.push(FE_WARN_AXR_OPTIONS_MISSING.to_string());
  }
  files.push(BundleFile {
    archive_path: FE_BUNDLE_AXR_PARTIAL_PATH.to_string(),
    bytes: render_mm_options_fragment(&axr_pairs),
    mode: BundleFileMode::MergeKeys {
      target: format!("configs/{}", FE_AXR_OPTIONS_LTX),
      section: FE_AXR_OPTIONS_SECTION.to_string(),
    },
  });

  let all_factions = faction_sections(&config_text);
  let summary = BundleSummary {
    factions_total: all_factions.len() as u32,
    factions_created: all_factions.iter().filter(|(_, created)| *created).count() as u32,
    custom_armament: custom_armament.into_iter().map(|(n, _)| n).collect(),
    custom_squad_sizes: custom_squad_sizes.into_iter().map(|(n, _)| n).collect(),
    has_relations,
    has_population,
    has_point_types,
  };

  Ok(Some(CollectedState { files, warnings, summary }))
}

/// Synthetic bundle equivalent to the editor's in-game "reset all": the
/// default config as the live config, no custom files, and the axr keys the
/// editor writes on reset (plan §4.6) for factions found in the default
/// config. The 4 Common-tab flags are left untouched — the in-game reset
/// (`actions.script` `OnMsgResetYes`) never touches them either.
fn default_files(game_root: &Path) -> Result<Vec<BundleFile>> {
  let configs_dir = game_root.join(GAMEDATA_DIR).join(CONFIGS_DIR);
  let default_path = configs_dir.join(FE_DEFAULT_CONFIG_LTX);
  let default_bytes = fs::read(&default_path).with_context(|| format!("read {}", default_path.display()))?;
  let default_text = String::from_utf8_lossy(&default_bytes).into_owned();

  let mut files = vec![BundleFile {
    archive_path: FE_BUNDLE_CONFIG_PATH.to_string(),
    bytes: default_bytes,
    mode: BundleFileMode::Replace { target: format!("configs/{}", FE_CONFIG_LTX) },
  }];

  let axr_path = configs_dir.join(FE_AXR_OPTIONS_LTX);
  let mut axr_pairs = Vec::new();
  if let Some(axr) = AlifeConfig::load(&axr_path)? {
    let existing: HashSet<String> = axr.get_section(FE_AXR_OPTIONS_SECTION).into_iter().map(|(k, _)| k).collect();
    for (faction, _) in faction_sections(&default_text) {
      let defaults = [
        (format!("behavior_{}", faction), "proffi"),
        (format!("enable_respawn_{}", faction), "true"),
        (format!("leaders_faction_{}_bahavior", faction), "none"),
      ];
      for (key, value) in defaults {
        if existing.contains(&key) {
          axr_pairs.push((key, value.to_string()));
        }
      }
    }
  }
  files.push(BundleFile {
    archive_path: FE_BUNDLE_AXR_PARTIAL_PATH.to_string(),
    bytes: render_mm_options_fragment(&axr_pairs),
    mode: BundleFileMode::MergeKeys {
      target: format!("configs/{}", FE_AXR_OPTIONS_LTX),
      section: FE_AXR_OPTIONS_SECTION.to_string(),
    },
  });

  Ok(files)
}

fn axr_faction_keys(faction: &str) -> [String; 3] {
  [
    format!("behavior_{}", faction),
    format!("enable_respawn_{}", faction),
    format!("leaders_faction_{}_bahavior", faction),
  ]
}

/// `(stem, bytes)` of every `<name>.ltx` in `dir`, sorted by name. Empty if
/// the directory does not exist, or exists but is not actually a directory
/// (a corrupt install should not crash a bundle read — see
/// `apply_files`'s matching cleanup-loop check below). A name that does not
/// match the editor's own naming (`is_valid_custom_file_stem`) is skipped
/// with a log warning.
fn read_custom_dir(dir: &Path) -> Result<Vec<(String, Vec<u8>)>> {
  let mut out = Vec::new();
  if !dir.is_dir() {
    return Ok(out);
  }
  for entry in fs::read_dir(dir).with_context(|| format!("read_dir {}", dir.display()))? {
    let entry = entry?;
    let path = entry.path();
    if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("ltx") {
      continue;
    }
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
    if !is_valid_custom_file_stem(stem) {
      log::warn!("faction_settings: skipping unexpected file in {}: {:?}", dir.display(), path.file_name());
      continue;
    }
    let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    out.push((stem.to_string(), bytes));
  }
  out.sort_by(|a, b| a.0.cmp(&b.0));
  Ok(out)
}

// ---------------------------------------------------------------------------
// Packing / reading the archive
// ---------------------------------------------------------------------------

fn write_bundle(state: &CollectedState, name: &str, description: &str, author: &str, dest_path: &Path) -> Result<()> {
  let name = name.trim();
  if name.is_empty() {
    bail!("bundle name must not be empty");
  }
  if name.chars().count() > FE_MAX_NAME_LEN {
    bail!("bundle name too long");
  }
  if description.chars().count() > FE_MAX_DESC_LEN {
    bail!("bundle description too long");
  }
  if author.chars().count() > FE_MAX_AUTHOR_LEN {
    bail!("bundle author too long");
  }

  let mut manifest_files = Vec::with_capacity(state.files.len());
  for f in &state.files {
    let (mode, target) = match &f.mode {
      BundleFileMode::Replace { target } => ("replace", Some(target.clone())),
      BundleFileMode::MergeKeys { target, .. } => ("merge_keys", Some(target.clone())),
    };
    manifest_files.push(BundleManifestFile {
      path: f.archive_path.clone(),
      mode: mode.to_string(),
      size: f.bytes.len() as u64,
      sha256: sha256_bytes(&f.bytes),
      target,
    });
  }

  let manifest = BundleManifest {
    schema: FE_BUNDLE_SCHEMA,
    kind: FE_BUNDLE_KIND.to_string(),
    name: name.to_string(),
    description: description.trim().to_string(),
    author: author.trim().to_string(),
    created_at: chrono::Utc::now().to_rfc3339(),
    files: manifest_files,
    summary: state.summary.clone(),
  };
  let manifest_json = serde_json::to_vec_pretty(&manifest).context("serialize manifest.json")?;

  if let Some(parent) = dest_path.parent() {
    fs::create_dir_all(parent).with_context(|| format!("create_dir_all {}", parent.display()))?;
  }
  let tmp_path = dest_path.with_extension(format!("{}.{}.tmp", FE_BUNDLE_EXT, std::process::id()));
  {
    let file = fs::File::create(&tmp_path).with_context(|| format!("create {}", tmp_path.display()))?;
    let mut zip = ZipWriter::new(file);
    let options: FileOptions<'_, ()> = FileOptions::default().compression_method(CompressionMethod::Deflated);

    zip.start_file(MANIFEST_NAME, options).context("start manifest.json entry")?;
    zip.write_all(&manifest_json).context("write manifest.json entry")?;

    for f in &state.files {
      zip.start_file(&f.archive_path, options).with_context(|| format!("start entry {}", f.archive_path))?;
      zip.write_all(&f.bytes).with_context(|| format!("write entry {}", f.archive_path))?;
    }
    zip.finish().context("finish zip")?;
  }
  fs::rename(&tmp_path, dest_path).with_context(|| format!("rename {} -> {}", tmp_path.display(), dest_path.display()))?;

  Ok(())
}

/// Open a `.gwfe`, validate it (`inspect_bundle`), read every declared file
/// into memory and verify its size + sha256, then validate the domain rules
/// that only make sense with the actual bytes in hand: the config carries a
/// `[stalker]` section (otherwise the editor's own bootstrap would blank it
/// on next open — `state_config.script` ~line 443), and the axr fragment only
/// touches keys the editor itself writes.
fn read_and_verify_bundle(bundle_path: &Path) -> Result<(BundleManifest, Vec<BundleFile>)> {
  let inspected = inspect_bundle(bundle_path, None)?;
  let manifest = inspected.manifest;

  let file = fs::File::open(bundle_path).with_context(|| format!("open {}", bundle_path.display()))?;
  let mut archive = ZipArchive::new(file).map_err(|_| anyhow!("{}", FE_ERR_BUNDLE_NOT_ZIP))?;

  let mut files = Vec::with_capacity(manifest.files.len());
  for entry in &manifest.files {
    let zip_entry = archive.by_name(&entry.path).map_err(|_| anyhow!("{}: {}", FE_ERR_BUNDLE_NO_MANIFEST, entry.path))?;
    if zip_entry.size() != entry.size {
      bail!("{}: {}", FE_ERR_BUNDLE_HASH_MISMATCH, entry.path);
    }
    // Bounded by the DECLARED size, not the central-directory one: a zip
    // bomb can lie about `size()`, so never `read_to_end` an entry unbounded.
    let bytes = read_entry_bounded(zip_entry, entry.size).with_context(|| format!("read {}", entry.path))?;
    if bytes.len() as u64 != entry.size || sha256_bytes(&bytes) != entry.sha256 {
      bail!("{}: {}", FE_ERR_BUNDLE_HASH_MISMATCH, entry.path);
    }

    // Mode and target come from the whitelisted `path` only (validated in
    // inspect_bundle) — the manifest's `target` is never used as a path.
    let (expected_mode, target) =
      expected_mode_and_target(&entry.path).ok_or_else(|| anyhow!("{}: {}", FE_ERR_BUNDLE_UNKNOWN_FILE, entry.path))?;
    let mode = if expected_mode == "merge_keys" {
      BundleFileMode::MergeKeys { target, section: FE_AXR_OPTIONS_SECTION.to_string() }
    } else {
      BundleFileMode::Replace { target }
    };

    files.push(BundleFile { archive_path: entry.path.clone(), bytes, mode });
  }

  let config = files
    .iter()
    .find(|f| f.archive_path == FE_BUNDLE_CONFIG_PATH)
    .ok_or_else(|| anyhow!("{}", FE_ERR_BUNDLE_INVALID_CONFIG))?;
  let has_stalker_section = String::from_utf8_lossy(&config.bytes)
    .lines()
    .any(|l| parse_section_header(l).map(|s| s.eq_ignore_ascii_case("stalker")).unwrap_or(false));
  if !has_stalker_section {
    bail!("{}", FE_ERR_BUNDLE_INVALID_CONFIG);
  }

  if let Some(axr) = files.iter().find(|f| matches!(f.mode, BundleFileMode::MergeKeys { .. })) {
    let text = String::from_utf8_lossy(&axr.bytes);
    for (key, _) in parse_mm_options_fragment(&text) {
      if !is_allowed_axr_key(&key) {
        bail!("{}: {}", FE_ERR_BUNDLE_INVALID_AXR_KEYS, key);
      }
    }
  }

  Ok((manifest, files))
}

// ---------------------------------------------------------------------------
// Applying to disk
// ---------------------------------------------------------------------------

fn apply_with_backup(game_root: &Path, backups_dir: &Path, files: &[BundleFile]) -> Result<FactionApplyResult> {
  let backup_state = resolved_from_disk(game_root)?;
  let backup_path = match &backup_state {
    Some(state) => {
      fs::create_dir_all(backups_dir).with_context(|| format!("create_dir_all {}", backups_dir.display()))?;
      // Milliseconds in the name: two applies within one second must not
      // overwrite each other's backup. Description stays empty — UI text is
      // localized on the frontend, not baked into a file.
      let path = backups_dir.join(format!("{}.{}", chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f"), FE_BUNDLE_EXT));
      write_bundle(state, "backup", "", "", &path)?;
      rotate_backups(backups_dir)?;
      Some(path)
    }
    None => None,
  };

  match apply_files(game_root, files) {
    Ok(mut warnings) => {
      warnings.sort();
      warnings.dedup();
      Ok(FactionApplyResult {
        outcome: ApplyOutcome::Applied,
        warnings,
        backup_path: backup_path.map(|p| p.to_string_lossy().into_owned()),
      })
    }
    Err(apply_err) => {
      log::error!("faction_settings: apply failed, rolling back: {}", apply_err);
      // `warnings[0]` carries the reason, prefixed with the FE_ERR_APPLY_FAILED
      // code so the frontend can localize the headline and show the detail.
      let reason = format!("{}: {}", FE_ERR_APPLY_FAILED, apply_err);
      let Some(state) = backup_state else {
        // Nothing to roll back to: the editor had never been saved, so there
        // was no previous state — whatever was written stays as-is and the
        // editor will treat it as its starting point. Reported as Failed.
        return Ok(FactionApplyResult {
          outcome: ApplyOutcome::Failed,
          warnings: vec![reason],
          backup_path: None,
        });
      };
      match apply_files(game_root, &state.files) {
        Ok(_) => Ok(FactionApplyResult {
          outcome: ApplyOutcome::RolledBack,
          warnings: vec![reason],
          backup_path: backup_path.map(|p| p.to_string_lossy().into_owned()),
        }),
        Err(rollback_err) => {
          log::error!("faction_settings: rollback failed too: {}", rollback_err);
          bail!("{}: apply failed ({}), and rollback also failed ({})", FE_ERR_ROLLBACK_FAILED, apply_err, rollback_err);
        }
      }
    }
  }
}

/// Write `files` onto `game_root`, deleting managed files/dirs the set does
/// not carry (full-replace semantics — see the plan §2). Returns non-fatal
/// warnings (e.g. `axr_options.ltx` missing).
fn apply_files(game_root: &Path, files: &[BundleFile]) -> Result<Vec<String>> {
  let configs_dir = game_root.join(GAMEDATA_DIR).join(CONFIGS_DIR);
  let mut warnings = Vec::new();

  for f in files {
    let BundleFileMode::Replace { target } = &f.mode else { continue };
    let target_path = configs_dir.join(rel_to_path(target.trim_start_matches("configs/")));
    if let Some(parent) = target_path.parent() {
      fs::create_dir_all(parent).with_context(|| format!("create_dir_all {}", parent.display()))?;
    }
    atomic_write_bytes(&target_path, &f.bytes).with_context(|| format!("write {}", target_path.display()))?;
  }

  // Compared with the `configs/` prefix stripped on both sides, so a target
  // spelled either way can never be mistaken for "absent" and deleted right
  // after it was written.
  let present: HashSet<&str> = files
    .iter()
    .filter_map(|f| match &f.mode {
      BundleFileMode::Replace { target } => Some(strip_configs_prefix(target)),
      _ => None,
    })
    .collect();

  for rel in [FE_GAME_RELATIONS_CUSTOM, FE_DEFAULT_CUSTOM_SIM, FE_SIM_OBJECTS_PROPS_CUSTOM] {
    if !present.contains(rel) {
      let path = configs_dir.join(rel_to_path(rel));
      if path.exists() {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
      }
    }
  }

  for dir in [FE_ARMAMENT_CUSTOM_DIR, FE_SQUAD_DESCR_CUSTOM_DIR] {
    let dir_path = configs_dir.join(rel_to_path(dir));
    // Not a directory (missing, or unexpectedly a file on a corrupt
    // install) — nothing to clean up inside it; a write into it, if the
    // bundle needs one, fails on its own `create_dir_all` below instead.
    if !dir_path.is_dir() {
      continue;
    }
    for entry in fs::read_dir(&dir_path).with_context(|| format!("read_dir {}", dir_path.display()))? {
      let entry = entry?;
      let path = entry.path();
      // Same filter as `read_custom_dir`: only files the backup could have
      // captured are deleted, so a rollback can always restore them.
      // Anything else (notes.txt, Dolg.ltx, *.bak) is left alone.
      if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("ltx") {
        continue;
      }
      if !path.file_stem().and_then(|s| s.to_str()).map(is_valid_custom_file_stem).unwrap_or(false) {
        continue;
      }
      let rel = format!("{}/{}", dir, path.file_name().and_then(|n| n.to_str()).unwrap_or_default());
      if !present.contains(rel.as_str()) {
        fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
      }
    }
  }

  if let Some(f) = files.iter().find(|f| matches!(f.mode, BundleFileMode::MergeKeys { .. })) {
    let BundleFileMode::MergeKeys { target, section } = &f.mode else { unreachable!() };
    let target_path = configs_dir.join(rel_to_path(target.trim_start_matches("configs/")));
    match AlifeConfig::load(&target_path)? {
      None => warnings.push(FE_WARN_AXR_OPTIONS_MISSING.to_string()),
      Some(mut axr) => {
        let pairs = parse_mm_options_fragment(&String::from_utf8_lossy(&f.bytes));
        if !pairs.is_empty() {
          if !axr.has_section(section) {
            // Section header missing — do not invent it; the editor creates
            // it itself. An existing-but-empty section is fine: keys are
            // appended to it by `set_in_section`.
            warnings.push(FE_WARN_AXR_OPTIONS_SECTION_MISSING.to_string());
          } else {
            for (key, value) in pairs {
              axr.set_in_section(section, &key, &value);
            }
            axr.save().with_context(|| format!("save {}", target_path.display()))?;
          }
        }
      }
    }
  }

  Ok(warnings)
}

fn rotate_backups(backups_dir: &Path) -> Result<()> {
  let mut entries: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(backups_dir)
    .with_context(|| format!("read_dir {}", backups_dir.display()))?
    .filter_map(|e| e.ok())
    .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some(FE_BUNDLE_EXT))
    .filter_map(|e| e.metadata().ok().and_then(|m| m.modified().ok()).map(|t| (t, e.path())))
    .collect();
  entries.sort_by_key(|(t, _)| *t);
  while entries.len() > FE_MAX_BACKUPS {
    let (_, path) = entries.remove(0);
    let _ = fs::remove_file(&path);
  }
  Ok(())
}

// ---------------------------------------------------------------------------
// Small parsing / formatting helpers
// ---------------------------------------------------------------------------

fn sha256_bytes(bytes: &[u8]) -> String {
  let mut hasher = Sha256::new();
  hasher.update(bytes);
  hex_lower(&hasher.finalize())
}

/// Read a zip entry into memory, refusing anything longer than `limit`
/// bytes. The central-directory `size()` is attacker-controlled (zip bomb),
/// so it is never trusted for pre-allocation or as the read bound.
fn read_entry_bounded<R: Read>(entry: R, limit: u64) -> Result<Vec<u8>> {
  let mut bytes = Vec::new();
  entry.take(limit + 1).read_to_end(&mut bytes)?;
  if bytes.len() as u64 > limit {
    bail!("{}", FE_ERR_BUNDLE_TOO_LARGE);
  }
  Ok(bytes)
}

/// The only `(mode, target)` a whitelisted archive path may have. `None` —
/// the path is not whitelisted at all. `target` is always spelled with the
/// `configs/` prefix, i.e. identical to the archive path for `replace`.
fn expected_mode_and_target(archive_path: &str) -> Option<(&'static str, String)> {
  if !is_whitelisted_archive_path(archive_path) {
    return None;
  }
  if archive_path == FE_BUNDLE_AXR_PARTIAL_PATH {
    Some(("merge_keys", format!("configs/{}", FE_AXR_OPTIONS_LTX)))
  } else {
    Some(("replace", archive_path.to_string()))
  }
}

fn strip_configs_prefix(target: &str) -> &str {
  target.strip_prefix("configs/").unwrap_or(target)
}

fn rel_to_path(rel: &str) -> PathBuf {
  let mut p = PathBuf::new();
  for part in rel.split('/') {
    p.push(part);
  }
  p
}

fn paths_equal(saved: &str, current: &Path) -> bool {
  let current = current.to_string_lossy();
  if cfg!(windows) {
    saved.eq_ignore_ascii_case(&current)
  } else {
    saved == current
  }
}

fn is_valid_custom_file_stem(name: &str) -> bool {
  !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Every path a `.gwfe` archive may contain besides `manifest.json`
/// (plan §3.2).
fn is_whitelisted_archive_path(path: &str) -> bool {
  const FIXED: &[&str] = &[
    FE_BUNDLE_CONFIG_PATH,
    FE_BUNDLE_AXR_PARTIAL_PATH,
    FE_BUNDLE_RELATIONS_PATH,
    FE_BUNDLE_DEFAULT_CUSTOM_PATH,
    FE_BUNDLE_SIM_PROPS_CUSTOM_PATH,
  ];
  if FIXED.contains(&path) {
    return true;
  }
  for dir in [FE_BUNDLE_ARMAMENT_CUSTOM_DIR, FE_BUNDLE_SQUAD_DESCR_CUSTOM_DIR] {
    let prefix = format!("{}/", dir);
    if let Some(stem) = path.strip_prefix(&prefix).and_then(|rest| rest.strip_suffix(".ltx")) {
      if !stem.is_empty() && !stem.contains('/') && is_valid_custom_file_stem(stem) {
        return true;
      }
    }
  }
  false
}

/// `axr_options.partial.ltx` may only carry keys the faction editor itself
/// writes to `[mm_options]` (plan §3.4 / §4.5).
fn is_allowed_axr_key(key: &str) -> bool {
  if FE_AXR_COMMON_FLAGS.contains(&key) {
    return true;
  }
  if let Some(rest) = key.strip_prefix("behavior_") {
    return !rest.is_empty();
  }
  if let Some(rest) = key.strip_prefix("enable_respawn_") {
    return !rest.is_empty();
  }
  if let Some(rest) = key.strip_prefix("leaders_faction_") {
    return rest.len() > "_bahavior".len() && rest.ends_with("_bahavior");
  }
  false
}

/// `[section]` -> `section`, mirroring `AlifeConfig`'s header parser but over
/// already-decoded UTF-8 text (the editor's own config files are UTF-8,
/// unlike `axr_options.ltx` — see the plan §1.4).
fn parse_section_header(line: &str) -> Option<&str> {
  let trimmed = line.trim();
  if !trimmed.starts_with('[') {
    return None;
  }
  let close = trimmed.find(']')?;
  Some(trimmed[1..close].trim())
}

/// Top-level faction sections of `faction_editor_config.ltx` /
/// `..._default_config.ltx`: only a genuine faction section carries an
/// `isCreated` key (rank, visuals and name-pool sections never do — see the
/// plan §1.1). Returns `(section_name, is_created)`.
fn faction_sections(content: &str) -> Vec<(String, bool)> {
  let mut out: Vec<(String, bool)> = Vec::new();
  let mut current: Option<(String, bool)> = None;
  let mut current_has_flag = false;

  for line in content.lines() {
    if let Some(name) = parse_section_header(line) {
      if let Some((name, created)) = current.take() {
        if current_has_flag {
          out.push((name, created));
        }
      }
      current = Some((name.to_string(), false));
      current_has_flag = false;
      continue;
    }
    let Some((_, created)) = current.as_mut() else { continue };
    let payload = match line.find(';') {
      Some(pos) => &line[..pos],
      None => line,
    };
    let Some(eq) = payload.find('=') else { continue };
    let key = payload[..eq].trim();
    if key.eq_ignore_ascii_case(FE_IS_CREATED_KEY) {
      let value = payload[eq + 1..].trim();
      *created = value.eq_ignore_ascii_case("true") || value == "1";
      current_has_flag = true;
    }
  }
  if let Some((name, created)) = current.take() {
    if current_has_flag {
      out.push((name, created));
    }
  }
  out
}

/// Extract `key = value` pairs from a `[mm_options]`-only ltx fragment (the
/// `axr_options.partial.ltx` entry of a bundle).
fn parse_mm_options_fragment(content: &str) -> Vec<(String, String)> {
  let mut out = Vec::new();
  let mut in_section = false;
  for line in content.lines() {
    if let Some(name) = parse_section_header(line) {
      in_section = name.eq_ignore_ascii_case(FE_AXR_OPTIONS_SECTION);
      continue;
    }
    if !in_section {
      continue;
    }
    let payload = match line.find(';') {
      Some(pos) => &line[..pos],
      None => line,
    };
    let Some(eq) = payload.find('=') else { continue };
    let key = payload[..eq].trim();
    let value = payload[eq + 1..].trim();
    if key.is_empty() {
      continue;
    }
    out.push((key.to_string(), value.to_string()));
  }
  out
}

/// Engine-style formatting (`CInifile::save_as`): 8-space indent, key padded
/// to 32 columns, ` = `, CRLF. Values are always plain ASCII tokens (faction
/// ids, `proffi`, `none`, `true`/`false`), so plain bytes are safe here.
const FRAGMENT_KEY_INDENT: &str = "        ";
const FRAGMENT_KEY_WIDTH: usize = 32;

fn render_mm_options_fragment(pairs: &[(String, String)]) -> Vec<u8> {
  let mut out = String::new();
  out.push_str("[mm_options]\r\n");
  for (key, value) in pairs {
    out.push_str(&format!("{}{:<width$} = {}\r\n", FRAGMENT_KEY_INDENT, key, value, width = FRAGMENT_KEY_WIDTH));
  }
  out.into_bytes()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
  use super::*;

  fn unique_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gw_fe_test_{}_{}", name, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
  }

  const SAMPLE_CONFIG: &str = "[stalker]\r\n\
        isCreated                        = true\r\n\
        isEnabled                        = true\r\n\
\r\n\
[dolg]\r\n\
        isCreated                        = true\r\n\
        power                             = 10\r\n\
\r\n\
[monolith]\r\n\
        isCreated                        = false\r\n\
\r\n\
[dolg_veteran]\r\n\
        min_reputation                   = 100\r\n";

  const SAMPLE_AXR_OPTIONS: &str = "[graphics]\r\n        vid_mode                          = 1920x1080\r\n\r\n[mm_options]\r\n        behavior_stalker                 = proffi\r\n        behavior_dolg                    = proffi\r\n        enable_respawn_dolg              = true\r\n        leaders_faction_dolg_bahavior    = none\r\n        enable_events_without_player     = true\r\n        enable_events_with_azazel_mode   = true\r\n        enable_change_beh_factions       = true\r\n        enable_respawn_factions          = true\r\n        some_unrelated_key               = keep_me\r\n";

  fn make_fixture_game_root(name: &str) -> PathBuf {
    let root = unique_dir(name);
    let configs = root.join("gamedata").join("configs");
    fs::create_dir_all(&configs).unwrap();
    fs::write(configs.join(FE_CONFIG_LTX), SAMPLE_CONFIG.as_bytes()).unwrap();
    fs::write(configs.join(FE_DEFAULT_CONFIG_LTX), SAMPLE_CONFIG.as_bytes()).unwrap();
    let axr_bytes = crate::utils::encoding::encode_cp1251(SAMPLE_AXR_OPTIONS).unwrap();
    fs::write(configs.join(FE_AXR_OPTIONS_LTX), axr_bytes).unwrap();
    root
  }

  #[test]
  fn roundtrip_export_then_apply_is_byte_identical() {
    let src_root = make_fixture_game_root("roundtrip_src");
    let dst_root = unique_dir("roundtrip_dst");
    fs::create_dir_all(dst_root.join("gamedata").join("configs")).unwrap();
    // apply_files patches an existing axr_options.ltx; seed the destination
    // with the same file the source started from.
    fs::copy(
      src_root.join("gamedata/configs").join(FE_AXR_OPTIONS_LTX),
      dst_root.join("gamedata/configs").join(FE_AXR_OPTIONS_LTX),
    )
    .unwrap();

    let armament_dir = src_root.join("gamedata/configs").join(rel_to_path(FE_ARMAMENT_CUSTOM_DIR));
    fs::create_dir_all(&armament_dir).unwrap();
    fs::write(armament_dir.join("dolg.ltx"), b"[dolg_veteran_1]\r\n        rank = veteran\r\n").unwrap();

    let bundle_path = unique_dir("roundtrip_bundle").join("test.gwfe");
    export_bundle(&src_root, "Test preset", "desc", "author", &bundle_path).unwrap();

    let backups = unique_dir("roundtrip_backups");
    let result = apply_bundle(&bundle_path, &dst_root, &backups).unwrap();
    assert_eq!(result.outcome, ApplyOutcome::Applied);

    let src_config = fs::read(src_root.join("gamedata/configs").join(FE_CONFIG_LTX)).unwrap();
    let dst_config = fs::read(dst_root.join("gamedata/configs").join(FE_CONFIG_LTX)).unwrap();
    assert_eq!(src_config, dst_config, "faction_editor_config.ltx must be byte-identical after round-trip");

    let dst_armament = fs::read(dst_root.join("gamedata/configs").join(rel_to_path(FE_ARMAMENT_CUSTOM_DIR)).join("dolg.ltx")).unwrap();
    assert_eq!(dst_armament, b"[dolg_veteran_1]\r\n        rank = veteran\r\n");

    // axr_options.ltx: the graphics section and the unrelated key must survive.
    let dst_axr = crate::utils::encoding::read_cp1251_file(dst_root.join("gamedata/configs").join(FE_AXR_OPTIONS_LTX)).unwrap();
    assert!(dst_axr.contains("vid_mode                          = 1920x1080"));
    assert!(dst_axr.contains("some_unrelated_key               = keep_me"));
    assert!(dst_axr.contains("behavior_dolg                    = proffi"));
  }

  #[test]
  fn rejects_path_outside_whitelist() {
    assert!(!is_whitelisted_archive_path("configs/system.ltx"));
    assert!(!is_whitelisted_archive_path("../../evil.ltx"));
    assert!(!is_whitelisted_archive_path("configs/misc/armament/custom/../../../evil.ltx"));
    assert!(is_whitelisted_archive_path(FE_BUNDLE_CONFIG_PATH));
    assert!(is_whitelisted_archive_path("configs/misc/armament/custom/dolg.ltx"));
  }

  #[test]
  fn rejects_wrong_kind_and_schema() {
    let bundle_path = unique_dir("wrong_kind").join("fake.gwfe");
    let file = fs::File::create(&bundle_path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options: FileOptions<'_, ()> = FileOptions::default();
    zip.start_file(MANIFEST_NAME, options).unwrap();
    zip.write_all(br#"{"schema":1,"kind":"something-else","name":"x","files":[]}"#).unwrap();
    zip.finish().unwrap();

    let err = inspect_bundle(&bundle_path, None).unwrap_err().to_string();
    assert!(err.contains(FE_ERR_BUNDLE_KIND), "unexpected error: {}", err);
  }

  #[test]
  fn rejects_tampered_hash() {
    let src_root = make_fixture_game_root("tamper_src");
    let bundle_path = unique_dir("tamper_bundle").join("test.gwfe");
    export_bundle(&src_root, "Test", "", "", &bundle_path).unwrap();

    // Re-zip the archive with a tampered config entry but the ORIGINAL manifest
    // (which still declares the original sha256/size).
    let mut archive = ZipArchive::new(fs::File::open(&bundle_path).unwrap()).unwrap();
    let mut manifest_bytes = Vec::new();
    archive.by_name(MANIFEST_NAME).unwrap().read_to_end(&mut manifest_bytes).unwrap();

    let tampered_path = unique_dir("tamper_out").join("tampered.gwfe");
    let file = fs::File::create(&tampered_path).unwrap();
    let mut zip = ZipWriter::new(file);
    let options: FileOptions<'_, ()> = FileOptions::default();
    zip.start_file(MANIFEST_NAME, options).unwrap();
    zip.write_all(&manifest_bytes).unwrap();
    zip.start_file(FE_BUNDLE_CONFIG_PATH, options).unwrap();
    zip.write_all(b"[stalker]\r\n        isCreated = true\r\n        TAMPERED = 1\r\n").unwrap();
    zip.start_file(FE_BUNDLE_AXR_PARTIAL_PATH, options).unwrap();
    zip.write_all(b"[mm_options]\r\n").unwrap();
    zip.finish().unwrap();

    let dst_root = unique_dir("tamper_dst");
    fs::create_dir_all(dst_root.join("gamedata/configs")).unwrap();
    let backups = unique_dir("tamper_backups");
    let err = apply_bundle(&tampered_path, &dst_root, &backups).unwrap_err().to_string();
    assert!(err.contains(FE_ERR_BUNDLE_HASH_MISMATCH), "unexpected error: {}", err);
  }

  #[test]
  fn axr_merge_keeps_other_keys_and_rejects_unknown_keys() {
    let mut pairs = vec![("behavior_dolg".to_string(), "proffi".to_string())];
    assert!(is_allowed_axr_key(&pairs[0].0));

    pairs.push(("vid_mode".to_string(), "1920x1080".to_string()));
    assert!(!is_allowed_axr_key("vid_mode"));

    assert!(is_allowed_axr_key("enable_respawn_factions"));
    assert!(is_allowed_axr_key("leaders_faction_dolg_bahavior"));
    assert!(!is_allowed_axr_key("leaders_faction_bahavior"));
    assert!(!is_allowed_axr_key("random_key"));
  }

  #[test]
  fn apply_removes_stale_custom_files_not_in_bundle() {
    let src_root = make_fixture_game_root("stale_src");
    let bundle_path = unique_dir("stale_bundle").join("test.gwfe");
    export_bundle(&src_root, "Test", "", "", &bundle_path).unwrap();

    let dst_root = unique_dir("stale_dst");
    let dst_configs = dst_root.join("gamedata/configs");
    fs::create_dir_all(dst_configs.join(rel_to_path(FE_ARMAMENT_CUSTOM_DIR))).unwrap();
    fs::write(dst_configs.join(rel_to_path(FE_ARMAMENT_CUSTOM_DIR)).join("stale.ltx"), b"[x]\r\n").unwrap();
    fs::create_dir_all(dst_configs.join(rel_to_path(FE_GAME_RELATIONS_CUSTOM)).parent().unwrap()).unwrap();
    fs::write(dst_configs.join(rel_to_path(FE_GAME_RELATIONS_CUSTOM)), b"; stale\r\n").unwrap();
    fs::copy(src_root.join("gamedata/configs").join(FE_AXR_OPTIONS_LTX), dst_configs.join(FE_AXR_OPTIONS_LTX)).unwrap();

    let backups = unique_dir("stale_backups");
    let result = apply_bundle(&bundle_path, &dst_root, &backups).unwrap();
    assert_eq!(result.outcome, ApplyOutcome::Applied);

    assert!(!dst_configs.join(rel_to_path(FE_ARMAMENT_CUSTOM_DIR)).join("stale.ltx").exists());
    assert!(!dst_configs.join(rel_to_path(FE_GAME_RELATIONS_CUSTOM)).exists());
  }

  #[test]
  fn apply_defaults_clears_custom_files_and_config() {
    let root = make_fixture_game_root("defaults");
    let configs = root.join("gamedata/configs");
    fs::create_dir_all(configs.join(rel_to_path(FE_ARMAMENT_CUSTOM_DIR))).unwrap();
    fs::write(configs.join(rel_to_path(FE_ARMAMENT_CUSTOM_DIR)).join("dolg.ltx"), b"[x]\r\n").unwrap();
    // Simulate the editor having changed the live config away from the default.
    fs::write(configs.join(FE_CONFIG_LTX), b"[stalker]\r\n        isCreated = true\r\n        CHANGED = 1\r\n").unwrap();

    let backups = unique_dir("defaults_backups");
    let result = apply_defaults(&root, &backups).unwrap();
    assert_eq!(result.outcome, ApplyOutcome::Applied);

    assert!(!configs.join(rel_to_path(FE_ARMAMENT_CUSTOM_DIR)).join("dolg.ltx").exists());
    let config_after = fs::read_to_string(configs.join(FE_CONFIG_LTX)).unwrap();
    assert!(!config_after.contains("CHANGED"));
    assert!(config_after.contains("isCreated"));
  }

  #[test]
  fn rollback_restores_previous_state_on_write_failure() {
    let src_root = make_fixture_game_root("rollback_src");
    let bundle_path = unique_dir("rollback_bundle").join("test.gwfe");
    export_bundle(&src_root, "Test", "", "", &bundle_path).unwrap();

    let dst_root = unique_dir("rollback_dst");
    let dst_configs = dst_root.join("gamedata/configs");
    fs::create_dir_all(&dst_configs).unwrap();
    fs::write(dst_configs.join(FE_CONFIG_LTX), b"[stalker]\r\n        isCreated = true\r\n        ORIGINAL = 1\r\n").unwrap();
    fs::copy(src_root.join("gamedata/configs").join(FE_AXR_OPTIONS_LTX), dst_configs.join(FE_AXR_OPTIONS_LTX)).unwrap();

    // Force apply_files to fail while writing the armament custom file: put a
    // regular FILE where the bundle needs to create the `armament/custom`
    // DIRECTORY, so `create_dir_all` errors out mid-apply.
    let armament_dir = src_root.join("gamedata/configs").join(rel_to_path(FE_ARMAMENT_CUSTOM_DIR));
    fs::create_dir_all(&armament_dir).unwrap();
    fs::write(armament_dir.join("dolg.ltx"), b"[x]\r\n").unwrap();
    // Re-export with the armament file included.
    export_bundle(&src_root, "Test", "", "", &bundle_path).unwrap();
    fs::create_dir_all(dst_configs.join("misc/armament")).unwrap();
    fs::write(dst_configs.join("misc/armament/custom"), b"blocker").unwrap(); // file, not dir

    let backups = unique_dir("rollback_backups");
    let result = apply_bundle(&bundle_path, &dst_root, &backups).unwrap();
    assert_eq!(result.outcome, ApplyOutcome::RolledBack);
    assert!(!result.warnings.is_empty());

    let restored = fs::read_to_string(dst_configs.join(FE_CONFIG_LTX)).unwrap();
    assert!(restored.contains("ORIGINAL"), "config must be restored to the pre-apply state");
  }

  /// Re-zip an exported bundle with a manifest transformed by `patch` and an
  /// optional extra undeclared entry.
  fn rebuild_bundle(src: &Path, dest: &Path, patch: impl Fn(&mut serde_json::Value), extra_entry: Option<(&str, &[u8])>) {
    let mut archive = ZipArchive::new(fs::File::open(src).unwrap()).unwrap();
    let mut manifest_bytes = Vec::new();
    archive.by_name(MANIFEST_NAME).unwrap().read_to_end(&mut manifest_bytes).unwrap();
    let mut manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes).unwrap();
    patch(&mut manifest);

    let file = fs::File::create(dest).unwrap();
    let mut zip = ZipWriter::new(file);
    let options: FileOptions<'_, ()> = FileOptions::default();
    zip.start_file(MANIFEST_NAME, options).unwrap();
    zip.write_all(&serde_json::to_vec(&manifest).unwrap()).unwrap();
    for i in 0..archive.len() {
      let mut entry = archive.by_index(i).unwrap();
      if entry.name() == MANIFEST_NAME {
        continue;
      }
      let name = entry.name().to_string();
      let mut bytes = Vec::new();
      entry.read_to_end(&mut bytes).unwrap();
      zip.start_file(&name, options).unwrap();
      zip.write_all(&bytes).unwrap();
    }
    if let Some((name, bytes)) = extra_entry {
      zip.start_file(name, options).unwrap();
      zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
  }

  #[test]
  fn manifest_target_is_ignored_so_traversal_cannot_escape_configs() {
    let src_root = make_fixture_game_root("traversal_src");
    let bundle_path = unique_dir("traversal_bundle").join("test.gwfe");
    export_bundle(&src_root, "Test", "", "", &bundle_path).unwrap();

    let evil_path = unique_dir("traversal_out").join("evil.gwfe");
    rebuild_bundle(
      &bundle_path,
      &evil_path,
      |m| {
        for f in m["files"].as_array_mut().unwrap() {
          if f["path"] == FE_BUNDLE_CONFIG_PATH {
            f["target"] = serde_json::Value::String("../../../evil_outside.ltx".to_string());
          }
        }
      },
      None,
    );

    let dst_root = unique_dir("traversal_dst");
    let dst_configs = dst_root.join("gamedata/configs");
    fs::create_dir_all(&dst_configs).unwrap();
    fs::copy(src_root.join("gamedata/configs").join(FE_AXR_OPTIONS_LTX), dst_configs.join(FE_AXR_OPTIONS_LTX)).unwrap();

    let backups = unique_dir("traversal_backups");
    let result = apply_bundle(&evil_path, &dst_root, &backups).unwrap();
    assert_eq!(result.outcome, ApplyOutcome::Applied);
    assert!(dst_configs.join(FE_CONFIG_LTX).exists(), "config must land at its whitelisted path");
    assert!(!dst_root.join("evil_outside.ltx").exists());
    assert!(!dst_root.parent().unwrap().join("evil_outside.ltx").exists());
  }

  #[test]
  fn inspect_rejects_wrong_mode_duplicates_and_undeclared_entries() {
    let src_root = make_fixture_game_root("mode_src");
    let bundle_path = unique_dir("mode_bundle").join("test.gwfe");
    export_bundle(&src_root, "Test", "", "", &bundle_path).unwrap();
    let out_dir = unique_dir("mode_out");

    // axr partial declared as `replace` → would overwrite axr_options.ltx wholesale.
    let wrong_mode = out_dir.join("wrong_mode.gwfe");
    rebuild_bundle(
      &bundle_path,
      &wrong_mode,
      |m| {
        for f in m["files"].as_array_mut().unwrap() {
          if f["path"] == FE_BUNDLE_AXR_PARTIAL_PATH {
            f["mode"] = serde_json::Value::String("replace".to_string());
          }
        }
      },
      None,
    );
    let err = inspect_bundle(&wrong_mode, None).unwrap_err().to_string();
    assert!(err.starts_with(FE_ERR_BUNDLE_UNKNOWN_FILE), "unexpected: {}", err);

    // Duplicate declaration.
    let dup = out_dir.join("dup.gwfe");
    rebuild_bundle(
      &bundle_path,
      &dup,
      |m| {
        let first = m["files"][0].clone();
        m["files"].as_array_mut().unwrap().push(first);
      },
      None,
    );
    let err = inspect_bundle(&dup, None).unwrap_err().to_string();
    assert!(err.starts_with(FE_ERR_BUNDLE_UNKNOWN_FILE), "unexpected: {}", err);

    // A whitelisted path that is in the zip but NOT declared in files[].
    let undeclared = out_dir.join("undeclared.gwfe");
    rebuild_bundle(&bundle_path, &undeclared, |_| {}, Some(("configs/misc/armament/custom/dolg.ltx", b"[x]\r\n")));
    let err = inspect_bundle(&undeclared, None).unwrap_err().to_string();
    assert!(err.starts_with(FE_ERR_BUNDLE_UNKNOWN_FILE), "unexpected: {}", err);

    // Declared but missing from the zip → refused at inspect, not at apply.
    let missing = out_dir.join("missing.gwfe");
    rebuild_bundle(
      &bundle_path,
      &missing,
      |m| {
        m["files"].as_array_mut().unwrap().push(serde_json::json!({
          "path": "configs/misc/armament/custom/dolg.ltx", "mode": "replace", "size": 5, "sha256": "00"
        }));
      },
      None,
    );
    let err = inspect_bundle(&missing, None).unwrap_err().to_string();
    assert!(err.starts_with(FE_ERR_BUNDLE_NO_MANIFEST), "unexpected: {}", err);

    // Config entry removed from files[] → invalid config.
    let no_config = out_dir.join("no_config.gwfe");
    rebuild_bundle(
      &bundle_path,
      &no_config,
      |m| {
        let files = m["files"].as_array_mut().unwrap();
        files.retain(|f| f["path"] != FE_BUNDLE_CONFIG_PATH);
      },
      None,
    );
    // The config entry is still in the zip but undeclared → UNKNOWN_FILE fires first.
    let err = inspect_bundle(&no_config, None).unwrap_err().to_string();
    assert!(err.starts_with(FE_ERR_BUNDLE_INVALID_CONFIG) || err.starts_with(FE_ERR_BUNDLE_UNKNOWN_FILE), "unexpected: {}", err);
  }

  #[test]
  fn apply_leaves_non_editor_files_in_custom_dirs_alone() {
    let src_root = make_fixture_game_root("keep_src");
    let bundle_path = unique_dir("keep_bundle").join("test.gwfe");
    export_bundle(&src_root, "Test", "", "", &bundle_path).unwrap();

    let dst_root = unique_dir("keep_dst");
    let dst_configs = dst_root.join("gamedata/configs");
    let armament = dst_configs.join(rel_to_path(FE_ARMAMENT_CUSTOM_DIR));
    fs::create_dir_all(&armament).unwrap();
    fs::write(armament.join("notes.txt"), b"keep").unwrap();
    fs::write(armament.join("Dolg.ltx"), b"keep").unwrap(); // uppercase: not an editor file
    fs::write(armament.join("stale.ltx"), b"[x]\r\n").unwrap();
    fs::copy(src_root.join("gamedata/configs").join(FE_AXR_OPTIONS_LTX), dst_configs.join(FE_AXR_OPTIONS_LTX)).unwrap();

    let result = apply_bundle(&bundle_path, &dst_root, &unique_dir("keep_backups")).unwrap();
    assert_eq!(result.outcome, ApplyOutcome::Applied);
    assert!(armament.join("notes.txt").exists());
    assert!(armament.join("Dolg.ltx").exists());
    assert!(!armament.join("stale.ltx").exists());
  }

  #[test]
  fn update_bundle_meta_changes_only_manifest_fields() {
    let src_root = make_fixture_game_root("meta_src");
    let bundle_path = unique_dir("meta_bundle").join("test.gwfe");
    export_bundle(&src_root, "Old name", "Old desc", "old author", &bundle_path).unwrap();

    update_bundle_meta(&bundle_path, "New name", "New desc", "old author").unwrap();

    let inspected = inspect_bundle(&bundle_path, None).unwrap();
    assert_eq!(inspected.manifest.name, "New name");
    assert_eq!(inspected.manifest.description, "New desc");
    assert_eq!(inspected.manifest.author, "old author");
    assert_eq!(inspected.manifest.files.len(), 2); // config + axr partial, no optional files

    let (_manifest, files) = read_and_verify_bundle(&bundle_path).unwrap();
    let config = files.iter().find(|f| f.archive_path == FE_BUNDLE_CONFIG_PATH).unwrap();
    assert_eq!(config.bytes, SAMPLE_CONFIG.as_bytes());
  }

  #[test]
  fn faction_sections_ignores_rank_and_visuals_sections() {
    let sections = faction_sections(SAMPLE_CONFIG);
    let names: Vec<&str> = sections.iter().map(|(n, _)| n.as_str()).collect();
    assert!(names.contains(&"dolg"));
    assert!(names.contains(&"monolith"));
    assert!(!names.contains(&"dolg_veteran"), "rank sections must not be treated as factions");
    let dolg_created = sections.iter().find(|(n, _)| n == "dolg").map(|(_, c)| *c);
    assert_eq!(dolg_created, Some(true));
    let monolith_created = sections.iter().find(|(n, _)| n == "monolith").map(|(_, c)| *c);
    assert_eq!(monolith_created, Some(false));
  }
}
