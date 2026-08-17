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
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Serialize;
use uuid::Uuid;

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

/// Collects the patch for every repo under `source_dir`.
///
/// `exclude_patterns` are glob patterns (relative to `source_dir`) for files
/// that should be skipped during collection (e.g. caches, logs, build artifacts).
pub fn collect_patch(source_dir: PathBuf, exclude_patterns: Vec<String>) -> Result<PatchCollectResult> {
  if !source_dir.is_dir() {
    bail!("source dir does not exist: {:?}", source_dir);
  }

  // Build a GlobSet for fast matching of excluded paths.
  let exclude_set = if exclude_patterns.is_empty() {
    None
  } else {
    let mut builder = GlobSetBuilder::new();
    for pat in &exclude_patterns {
      builder.add(Glob::new(pat).with_context(|| format!("invalid exclude pattern: '{}'", pat))?);
    }
    Some(builder.build().context("failed to build exclude glob set")?)
  };

  let patch_dir = patch_dir_root()?.join(format!("gw-patch-{}", Uuid::new_v4()));

  let mut result = PatchCollectResult {
    patch_dir: patch_dir.to_string_lossy().into_owned(),
    deleted_files: Vec::new(),
    base_tag: None,
    repos: Vec::new(),
    changed: 0,
    deleted: 0,
  };

  for repo_dir in find_git_roots(&source_dir) {
    let report = collect_repo(&repo_dir, &source_dir, &patch_dir, &mut result.deleted_files, exclude_set.as_ref());
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

  log::info!(
    "collect_patch done: repos: {}, changed: {}, deleted: {}, patch_dir: {:?}",
    result.repos.len(),
    result.changed,
    result.deleted,
    result.patch_dir
  );

  Ok(result)
}

/// Collects tag..HEAD changes of a single repository into the patch folder.
/// Files matching `exclude_set` (relative to `source_dir`) are silently skipped.
fn collect_repo(
  repo_dir: &Path,
  source_dir: &Path,
  patch_dir: &Path,
  deleted_files: &mut Vec<String>,
  exclude_set: Option<&GlobSet>,
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
        // Skip files matching exclude patterns.
        if exclude_set.map_or(false, |s| s.is_match(&rel)) {
          continue;
        }

        let src = repo_dir.join(new_path);
        if !src.is_file() {
          // Committed but missing on disk (e.g. sparse checkout) — skip with a note.
          report
            .message
            .get_or_insert_with(|| "some files are missing on disk and were skipped".to_string());
          continue;
        }

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
