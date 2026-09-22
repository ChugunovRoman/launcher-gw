//! Patch collection from game git repositories.
//!
//! Port of the reference script `patch/_collect_gw_diff.py` with improvements:
//! walks every git repository (main repo + nested repos/submodules) under the
//! selected game folder, diffs the latest tag reachable from HEAD against HEAD
//! (committed changes only, workdir/staged changes are ignored) and:
//!   - Added / Modified / Renamed(new side) / Copied / Typechange files are
//!     copied into a `patches` folder next to the launcher exe, preserving
//!     their relative paths;
//!   - Deleted files (and the old side of renames) are collected into a
//!     `deleted_files` list which later lands in the patch manifest.json.
//!
//! The "latest reachable tag" rule covers both the first patch after a full
//! release (tag of the full release) and subsequent patches (tag created by
//! the launcher when the previous patch was uploaded).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use serde::Serialize;
use uuid::Uuid;

use crate::consts::{FE_DEFAULT_CONFIG_REL_PATH, FE_PATCH_FRAGMENT_STAGING};
use crate::service::faction_patch::{self, FePatchFragment};

/// Per-repository outcome of the collection.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoPatchStatus {
  /// Repo had changes tag..HEAD, files were copied.
  Collected,
  /// Repo has no tags at all — cannot determine the diff base.
  NoTags,
  /// HEAD equals the base tag — nothing changed since the tag.
  NoChanges,
  /// git error while reading the repo.
  Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct RepoPatchReport {
  /// Path relative to the selected source dir, "" for the root repo.
  pub repo_rel_path: String,
  /// The tag used as the diff base ("" when unknown).
  pub base_tag: String,
  pub status: RepoPatchStatus,
  pub changed: u32,
  pub deleted: u32,
  /// Error/warning message when present.
  pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatchCollectResult {
  /// Folder with the copied patch files (`patches/` next to the launcher exe,
  /// relative layout preserved).
  pub patch_dir: String,
  /// Files to delete when applying the patch, `/`-separated, relative to the game root.
  pub deleted_files: Vec<String>,
  /// Base tag of the root repo — used as base_release_tag in the patch manifest.
  pub base_tag: Option<String>,
  pub repos: Vec<RepoPatchReport>,
  pub changed: u32,
  pub deleted: u32,
  /// Distinct faction-editor props whose values changed in the mod's
  /// reference config since the base tag. Shown to the developer for review
  /// before upload; empty when the patch changes no settings.
  pub fe_updated_fields: Vec<String>,
  /// How many `(section, key)` pairs the settings fragment carries.
  pub fe_fragment_entries: u32,
  /// True when the diff touches spawn/level files — old save games stop
  /// working after installing such a patch.
  pub breaks_saves: bool,
  /// Paths (relative to the game root, `/`-separated) that tripped the
  /// save-breaking markers, both changed and deleted. Kept for the developer
  /// UI; the manifest carries only the boolean.
  pub save_breaking_files: Vec<String>,
}

/// Finds all git repository roots under `base` (including `base` itself).
/// A repo is a directory containing a `.git` entry (dir for normal repos,
/// file for submodules). `.git` directories are never descended into.
pub(crate) fn find_git_roots(base: &Path) -> Vec<PathBuf> {
  let mut roots: Vec<PathBuf> = Vec::new();

  for entry in walkdir::WalkDir::new(base)
    .follow_links(false)
    .into_iter()
    .filter_entry(|e| e.file_name() != ".git")
    .filter_map(|e| e.ok())
  {
    if !entry.file_type().is_dir() {
      continue;
    }
    if entry.path().join(".git").exists() {
      roots.push(entry.path().to_path_buf());
    }
  }

  // Fewer path components first: the main repo before its nested repos.
  roots.sort_by_key(|p| p.components().count());
  roots
}

/// Returns the tag reachable from `head` whose target commit has the newest
/// commit time. Ties are resolved arbitrarily (first found wins).
fn latest_reachable_tag<'repo>(
  repo: &'repo git2::Repository,
  head: &git2::Commit<'repo>,
) -> Option<(String, git2::Commit<'repo>)> {
  let tags = repo.tag_names(None).ok()?;
  let mut best: Option<(i64, String, git2::Commit<'repo>)> = None;

  for name in tags.iter().flatten() {
    let ref_name = format!("refs/tags/{}", name);
    let Ok(reference) = repo.find_reference(&ref_name) else { continue };
    // Annotated tags point to a tag object — peel down to the commit.
    let Ok(tag_commit) = reference.peel_to_commit() else { continue };

    let is_reachable = tag_commit.id() == head.id()
      || repo.graph_descendant_of(head.id(), tag_commit.id()).unwrap_or(false);
    if !is_reachable {
      continue;
    }

    let time = tag_commit.time().seconds();
    match &best {
      Some((best_time, _, _)) if *best_time >= time => {}
      _ => best = Some((time, name.to_string(), tag_commit)),
    }
  }

  best.map(|(_, name, commit)| (name, commit))
}

/// Normalizes a path relative to the game root for the manifest:
/// forward slashes, lossy.
fn to_rel_slash(path: &Path) -> String {
  path.to_string_lossy().replace('\\', "/")
}

/// Outcome of tagging a single game repository.
#[derive(Debug, Clone, Serialize)]
pub struct RepoTagReport {
  /// Path relative to the game source dir, "" for the root repo.
  pub repo_rel_path: String,
  /// Tag created locally on HEAD.
  pub tagged: bool,
  /// Tag pushed to origin (via system `git`).
  pub pushed: bool,
  pub message: Option<String>,
}

/// Creates `tag_name` on HEAD of every repo under `source_dir` and pushes it
/// to origin. This anchors the "state at patch N" so the next collect diffs
/// from this tag. All repos are tagged (not only changed ones) to keep the
/// diff bases consistent across the whole game tree. Errors are collected
/// into per-repo reports and never abort the whole run.
pub fn tag_game_repos(source_dir: &Path, tag_name: &str) -> Vec<RepoTagReport> {
  let mut reports = Vec::new();

  for repo_dir in find_git_roots(source_dir) {
    let repo_rel_path = repo_dir
      .strip_prefix(source_dir)
      .map(|p| p.to_string_lossy().into_owned())
      .unwrap_or_default();

    let mut report = RepoTagReport {
      repo_rel_path,
      tagged: false,
      pushed: false,
      message: None,
    };

    let repo = match git2::Repository::open(&repo_dir) {
      Ok(repo) => repo,
      Err(e) => {
        report.message = Some(format!("cannot open repository: {}", e));
        reports.push(report);
        continue;
      }
    };

    let head = match repo.head().and_then(|r| r.peel_to_commit()) {
      Ok(commit) => commit,
      Err(e) => {
        report.message = Some(format!("cannot resolve HEAD: {}", e));
        reports.push(report);
        continue;
      }
    };

    // Already tagged (e.g. retry after a failed push) — skip creation.
    if repo.find_reference(&format!("refs/tags/{}", tag_name)).is_ok() {
      report.message = Some("tag already exists".to_string());
    } else {
      // Prefer the committer identity from git config, fall back to a fixed one.
      let signature = repo
        .signature()
        .or_else(|_| git2::Signature::now("GW Launcher", "launcher@globalwar.local"));
      let signature = match signature {
        Ok(s) => s,
        Err(e) => {
          report.message = Some(format!("cannot build signature: {}", e));
          reports.push(report);
          continue;
        }
      };

      let msg = format!("Patch {}", tag_name);
      if let Err(e) = repo.tag(tag_name, head.as_object(), &signature, &msg, false) {
        report.message = Some(format!("cannot create tag: {}", e));
        reports.push(report);
        continue;
      }
      report.tagged = true;
    }

    // Push via the system git binary — it reuses the developer's stored
    // credentials (credential manager / ssh agent) without extra setup.
    let push = std::process::Command::new("git")
      .arg("-C")
      .arg(&repo_dir)
      .args(["push", "origin", tag_name])
      .output();

    match push {
      Ok(out) if out.status.success() => {
        report.pushed = true;
      }
      Ok(out) => {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        report.message = Some(format!("git push failed: {}", stderr));
      }
      Err(e) => {
        report.message = Some(format!("cannot run git: {}", e));
      }
    }

    reports.push(report);
  }

  reports
}

/// Base dir for collected patches: a `patches` folder next to the launcher exe.
fn patch_dir_root() -> Result<PathBuf> {
  let exe_dir = std::env::current_exe()
    .context("cannot resolve launcher exe path")?
    .parent()
    .context("launcher exe has no parent dir")?
    .to_path_buf();
  let root = exe_dir.join("patches");
  fs::create_dir_all(&root).context("create patches dir")?;
  Ok(root)
}

/// GlobSet of the save-breaking path markers (`consts::SAVE_BREAKING_GLOBS`),
/// case-insensitive like the exclude masks: the game tree on Windows may carry
/// `Gamedata/` or `gamedata/` spellings.
pub(crate) fn build_save_breaking_set() -> GlobSet {
  let mut builder = GlobSetBuilder::new();
  for pat in crate::consts::SAVE_BREAKING_GLOBS {
    builder.add(
      GlobBuilder::new(pat)
        .case_insensitive(true)
        .build()
        .unwrap_or_else(|e| panic!("invalid save-breaking pattern '{}': {}", pat, e)),
    );
  }
  builder.build().unwrap_or_else(|e| panic!("cannot build save-breaking glob set: {}", e))
}

/// Records `rel` (already `/`-separated) when it matches a save-breaking
/// marker. Keeps the list deduplicated — a rename can trip both delta sides.
fn note_save_breaking(set: &GlobSet, rel_slash: &str, hits: &mut Vec<String>) {
  if set.is_match(rel_slash) && !hits.iter().any(|h| h.eq_ignore_ascii_case(rel_slash)) {
    hits.push(rel_slash.to_string());
  }
}

/// Save-breaking re-scan of a patch upload folder. `upload_patch` cannot trust
/// the collector's verdict alone: the folder may have been collected in an
/// earlier session or edited by hand. Walks the folder and the delete list
/// against the same markers. Cheap — patch folders are small, and the packer
/// reads these files right after anyway.
pub fn scan_save_breaking(patch_dir: &Path, deleted_files: &[String]) -> Vec<String> {
  let set = build_save_breaking_set();
  let mut hits: Vec<String> = Vec::new();

  for entry in walkdir::WalkDir::new(patch_dir)
    .follow_links(false)
    .into_iter()
    .filter_entry(|e| e.file_name() != crate::consts::PATCH_ARCHIVE_DIR)
    .filter_map(|e| e.ok())
  {
    if !entry.file_type().is_file() {
      continue;
    }
    if let Ok(rel) = entry.path().strip_prefix(patch_dir) {
      note_save_breaking(&set, &to_rel_slash(rel), &mut hits);
    }
  }

  for rel in deleted_files {
    note_save_breaking(&set, rel, &mut hits);
  }

  hits
}

/// Collects the patch for every repo under `source_dir`.
///
/// `exclude_patterns` are glob patterns (relative to `source_dir`) for files
/// that should be skipped during collection (e.g. caches, logs, build artifacts).
/// Builds the exclusion `GlobSet` for patch collection.
///
/// Matching is case-insensitive, exactly like the packer
/// (`handlers/compress.rs`): on Windows `Logs/` and `logs/` are the same
/// directory, and case-sensitive masks let logs and user dirs into the patch
/// (item 71). Returns `None` when there is nothing to exclude.
fn build_exclude_set(exclude_patterns: &[String]) -> Result<Option<GlobSet>> {
  if exclude_patterns.is_empty() {
    return Ok(None);
  }

  let mut builder = GlobSetBuilder::new();
  for pat in exclude_patterns {
    builder.add(
      GlobBuilder::new(pat)
        .case_insensitive(true)
        .build()
        .with_context(|| format!("invalid exclude pattern: '{}'", pat))?,
    );
  }

  Ok(Some(builder.build().context("failed to build exclude glob set")?))
}

pub fn collect_patch(source_dir: PathBuf, exclude_patterns: Vec<String>) -> Result<PatchCollectResult> {
  if !source_dir.is_dir() {
    bail!("source dir does not exist: {:?}", source_dir);
  }

  // Build a GlobSet for fast matching of excluded paths.
  let exclude_set = build_exclude_set(&exclude_patterns)?;
  let save_breaking_set = build_save_breaking_set();

  let patch_dir = patch_dir_root()?.join(format!("gw-patch-{}", Uuid::new_v4()));

  let mut result = PatchCollectResult {
    patch_dir: patch_dir.to_string_lossy().into_owned(),
    deleted_files: Vec::new(),
    base_tag: None,
    repos: Vec::new(),
    changed: 0,
    deleted: 0,
    fe_updated_fields: Vec::new(),
    fe_fragment_entries: 0,
    breaks_saves: false,
    save_breaking_files: Vec::new(),
  };
  let mut fe_fragment: Option<FePatchFragment> = None;

  for repo_dir in find_git_roots(&source_dir) {
    let report = collect_repo(
      &repo_dir,
      &source_dir,
      &patch_dir,
      &mut result.deleted_files,
      exclude_set.as_ref(),
      &mut fe_fragment,
      &save_breaking_set,
      &mut result.save_breaking_files,
    );
    log::debug!(
      "collect_patch repo {:?}: status={:?} changed={} deleted={}",
      report.repo_rel_path,
      report.status,
      report.changed,
      report.deleted
    );

    // base_release_tag for the patch manifest comes from the root repo.
    if report.repo_rel_path.is_empty() && !report.base_tag.is_empty() {
      result.base_tag = Some(report.base_tag.clone());
    }

    result.changed += report.changed;
    result.deleted += report.deleted;
    result.repos.push(report);
  }

  result.breaks_saves = !result.save_breaking_files.is_empty();
  if result.breaks_saves {
    log::warn!(
      "collect_patch: patch breaks existing saves, markers: {}",
      result.save_breaking_files.join(", ")
    );
  }

  // Stage the faction-editor settings fragment inside the patch folder. It
  // travels as an ordinary patch file and unpacks straight into the player's
  // `appdata/patches`, where it stays as the record of what this patch
  // changed. The patch tag is not known yet, so it is written under the
  // staging name; `upload_patch` renames it.
  if let Some(fragment) = fe_fragment.as_ref().filter(|f| !f.is_empty()) {
    let dir = crate::utils::patch_markers::patches_dir(&patch_dir);
    fs::create_dir_all(&dir).with_context(|| format!("create {:?}", dir))?;
    let path = dir.join(FE_PATCH_FRAGMENT_STAGING);
    fs::write(&path, faction_patch::render_fragment(fragment)).with_context(|| format!("write {:?}", path))?;
    result.fe_updated_fields = fragment.fields.clone();
    result.fe_fragment_entries = fragment.edits.len() as u32;
    log::info!(
      "collect_patch: faction editor settings fragment: {} prop(s), {} value(s)",
      result.fe_updated_fields.len(),
      result.fe_fragment_entries
    );
  }

  log::info!(
    "collect_patch done: repos: {}, changed: {}, deleted: {}, breaks_saves: {}, patch_dir: {:?}",
    result.repos.len(),
    result.changed,
    result.deleted,
    result.breaks_saves,
    result.patch_dir
  );

  Ok(result)
}

/// Diff the two committed versions of `faction_editor_default_config.ltx` a
/// delta points at.
///
/// Blobs come from git, not the working tree, so uncommitted edits never leak
/// into a patch — the same rule the rest of the collection follows.
fn diff_fe_default_config(repo: &git2::Repository, delta: &git2::DiffDelta<'_>) -> Result<FePatchFragment> {
  let old_blob = repo.find_blob(delta.old_file().id()).context("read the previous reference config")?;
  let new_blob = repo.find_blob(delta.new_file().id()).context("read the new reference config")?;

  // Tags older than the editor's UTF-8 switch hold a cp1251 config; the
  // decoder falls back so a patch based on one of those still diffs.
  let old_text = faction_patch::decode_config_text(old_blob.content());
  let new_text = faction_patch::decode_config_text(new_blob.content());

  Ok(faction_patch::diff_configs(&old_text, &new_text))
}

/// Collects tag..HEAD changes of a single repository into the patch folder.
/// Files matching `exclude_set` (relative to `source_dir`) are silently skipped.
fn collect_repo(
  repo_dir: &Path,
  source_dir: &Path,
  patch_dir: &Path,
  deleted_files: &mut Vec<String>,
  exclude_set: Option<&GlobSet>,
  fe_fragment: &mut Option<FePatchFragment>,
  save_breaking_set: &GlobSet,
  save_breaking_files: &mut Vec<String>,
) -> RepoPatchReport {
  let rel_prefix = repo_dir
    .strip_prefix(source_dir)
    .map(|p| p.to_path_buf())
    .unwrap_or_else(|_| PathBuf::new());
  let rel_prefix_str = rel_prefix.to_string_lossy().into_owned();

  let mut report = RepoPatchReport {
    repo_rel_path: rel_prefix_str.clone(),
    base_tag: String::new(),
    status: RepoPatchStatus::Collected,
    changed: 0,
    deleted: 0,
    message: None,
  };

  let repo = match git2::Repository::open(repo_dir) {
    Ok(repo) => repo,
    Err(e) => {
      report.status = RepoPatchStatus::Error;
      report.message = Some(format!("cannot open repository: {}", e));
      return report;
    }
  };

  let head = match repo.head().and_then(|r| r.peel_to_commit()) {
    Ok(commit) => commit,
    Err(e) => {
      report.status = RepoPatchStatus::Error;
      report.message = Some(format!("cannot resolve HEAD: {}", e));
      return report;
    }
  };

  let Some((tag_name, base_commit)) = latest_reachable_tag(&repo, &head) else {
    report.status = RepoPatchStatus::NoTags;
    return report;
  };
  report.base_tag = tag_name.clone();

  if base_commit.id() == head.id() {
    report.status = RepoPatchStatus::NoChanges;
    return report;
  }

  let base_tree = match base_commit.tree() {
    Ok(tree) => tree,
    Err(e) => {
      report.status = RepoPatchStatus::Error;
      report.message = Some(format!("cannot read base tree of tag '{}': {}", tag_name, e));
      return report;
    }
  };
  let head_tree = match head.tree() {
    Ok(tree) => tree,
    Err(e) => {
      report.status = RepoPatchStatus::Error;
      report.message = Some(format!("cannot read HEAD tree: {}", e));
      return report;
    }
  };

  let diff = match repo.diff_tree_to_tree(Some(&base_tree), Some(&head_tree), None) {
    Ok(diff) => diff,
    Err(e) => {
      report.status = RepoPatchStatus::Error;
      report.message = Some(format!("diff failed ({}..HEAD): {}", tag_name, e));
      return report;
    }
  };

  for delta in diff.deltas() {
    let status = delta.status();

    // Old side: deleted files and the old path of renames go to the delete list.
    if matches!(status, git2::Delta::Deleted | git2::Delta::Renamed) {
      if let Some(old_path) = delta.old_file().path() {
        let rel = rel_prefix.join(old_path);
        // Skip files matching exclude patterns.
        if exclude_set.map_or(false, |s| s.is_match(&rel)) {
          continue;
        }
        // A deleted level/spawn file breaks saves exactly like a changed one.
        note_save_breaking(save_breaking_set, &to_rel_slash(&rel), save_breaking_files);
        deleted_files.push(to_rel_slash(&rel));
        report.deleted += 1;
      }
    }

    // New side: copy the committed file content into the patch folder.
    if matches!(
      status,
      git2::Delta::Added
        | git2::Delta::Modified
        | git2::Delta::Renamed
        | git2::Delta::Copied
        | git2::Delta::Typechange
    ) {
      if let Some(new_path) = delta.new_file().path() {
        let rel = rel_prefix.join(new_path);

        // The mod's reference faction-editor config additionally produces the
        // settings fragment. Only a Modified delta can: on Added there is no
        // previous version to diff against, and shipping the whole reference
        // config as "changes" would overwrite every balance prop the player
        // ever tuned.
        if to_rel_slash(&rel) == FE_DEFAULT_CONFIG_REL_PATH && status == git2::Delta::Modified {
          match diff_fe_default_config(&repo, &delta) {
            Ok(fragment) => {
              log::info!("collect_patch: faction editor diff gave {} value(s)", fragment.edits.len());
              *fe_fragment = Some(fragment);
            }
            Err(e) => {
              log::warn!("collect_patch: cannot diff {}: {}", FE_DEFAULT_CONFIG_REL_PATH, e);
              report
                .message
                .get_or_insert_with(|| format!("faction editor settings were not collected: {}", e));
            }
          }
        }

        // Skip files matching exclude patterns.
        if exclude_set.map_or(false, |s| s.is_match(&rel)) {
          continue;
        }

        // Only files that actually ship can break saves — after the exclude
        // filter and the on-disk check, same conditions as the copy below.
        let src = repo_dir.join(new_path);
        if !src.is_file() {
          // Committed but missing on disk (e.g. sparse checkout) — skip with a note.
          report
            .message
            .get_or_insert_with(|| "some files are missing on disk and were skipped".to_string());
          continue;
        }
        note_save_breaking(save_breaking_set, &to_rel_slash(&rel), save_breaking_files);

        let dest = patch_dir.join(&rel);
        if let Some(parent) = dest.parent() {
          if let Err(e) = fs::create_dir_all(parent).context("create patch subfolder") {
            report.status = RepoPatchStatus::Error;
            report.message = Some(format!("cannot create {:?}: {}", parent, e));
            return report;
          }
        }
        if let Err(e) = fs::copy(&src, &dest).context("copy patch file") {
          report.status = RepoPatchStatus::Error;
          report.message = Some(format!("cannot copy {:?}: {}", src, e));
          return report;
        }

        report.changed += 1;
      }
    }
  }

  // Nothing changed after all (e.g. only mode changes) — treat as no changes.
  if report.changed == 0 && report.deleted == 0 {
    report.status = RepoPatchStatus::NoChanges;
  }

  report
}

#[cfg(test)]
mod tests {
  use super::*;

  /// Item 71: exclusion masks must ignore letter case, otherwise `Logs/` and
  /// `_APPDATA_/` slip into the collected patch.
  #[test]
  fn exclude_masks_are_case_insensitive() {
    let set = build_exclude_set(&["appdata/**".to_owned(), "**/*.log".to_owned()])
      .expect("glob set builds")
      .expect("non-empty pattern list yields a set");

    assert!(set.is_match(Path::new("appdata/logs/x.txt")));
    assert!(set.is_match(Path::new("AppData/logs/x.txt")));
    assert!(set.is_match(Path::new("APPDATA/logs/x.txt")));
    assert!(set.is_match(Path::new("bin/openxray.log")));
    assert!(set.is_match(Path::new("bin/OpenXRay.LOG")));
    assert!(!set.is_match(Path::new("gamedata/configs/x.ltx")));
  }

  #[test]
  fn empty_exclude_list_yields_no_set() {
    assert!(build_exclude_set(&[]).expect("ok").is_none());
  }

  /// Every save-breaking marker must match its intended spelling and ignore
  /// case, while neighbouring level files (level.prj, other .spawn files)
  /// stay neutral — a false positive would warn on every ordinary patch.
  #[test]
  fn save_breaking_markers_match_their_files_only() {
    let set = build_save_breaking_set();

    for hits in [
      "gamedata/spawns/all.spawn",
      "gamedata/levels/l01_escape/level.ai",
      "gamedata/levels/k01_marsh/level.game",
      "gamedata/levels/zaton/level.spawn",
      "GAMEDATA/Spawns/ALL.SPAWN",
      "Gamedata/Levels/L01_Escape/Level.AI",
    ] {
      assert!(set.is_match(hits), "{} must be a save-breaking marker", hits);
    }

    for neutral in [
      "gamedata/levels/l01_escape/level.prj",
      "gamedata/levels/l01_escape/level.detail",
      "gamedata/levels/l01_escape/level.ltx",
      "gamedata/configs/creatures/m_stalker.ltx",
      "gamedata/spawns/alife.psua", // not all.spawn
      "appdata/patches/x.faction_editor_patch.ltx",
    ] {
      assert!(!set.is_match(neutral), "{} must NOT be a save-breaking marker", neutral);
    }
  }

  /// The upload-time re-scan walks the folder and the delete list: deleted
  /// markers count the same as shipped ones, and unrelated files (the pack
  /// dir of a previous upload, the fe fragment) do not.
  #[test]
  fn scan_save_breaking_covers_files_and_deletions() {
    let root = std::env::temp_dir().join(format!("gw_scan_save_breaking_{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("gamedata/levels/l01_escape")).unwrap();
    fs::create_dir_all(root.join("gamedata/configs")).unwrap();
    fs::write(root.join("gamedata/levels/l01_escape/level.ai"), b"ai").unwrap();
    fs::write(root.join("gamedata/configs/weapons.ltx"), b"ltx").unwrap();

    let mut hits = scan_save_breaking(&root, &["gamedata/spawns/all.spawn".to_string()]);
    hits.sort();
    assert_eq!(
      hits,
      vec![
        "gamedata/levels/l01_escape/level.ai".to_string(),
        "gamedata/spawns/all.spawn".to_string(),
      ]
    );

    // No markers anywhere — empty result, including an empty delete list.
    fs::remove_dir_all(&root).unwrap();
    fs::create_dir_all(root.join("gamedata/configs")).unwrap();
    fs::write(root.join("gamedata/configs/weapons.ltx"), b"ltx").unwrap();
    assert!(scan_save_breaking(&root, &[]).is_empty());

    fs::remove_dir_all(&root).ok();
  }
}
