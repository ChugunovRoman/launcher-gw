use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use std::fs;

use bytes::Bytes;
use futures_util::Stream;
use serde::Serialize;
use tauri::Emitter;
use tokio::{fs::File, sync::broadcast, sync::Mutex};
use tokio_util::io::ReaderStream;

use crate::consts::{DEFAULT_BRANCH, MANIFEST_NAME};
use crate::handlers::compress::pack_split_archives;
use crate::handlers::dto::{PatchMeta, ReleaseManifestFile, UploadProgressPayload};
use crate::handlers::patch_install::{project_id_for, resolve_updates_project};
use crate::handlers::upload_v2::{UploadCancelMap, build_asset_url, make_tag_name};
use crate::providers::dto::CreateReleaseAsset;
use crate::service::main::Service;
use crate::utils::errors::log_full_error;
use crate::utils::patch_collect::{self, RepoTagReport};

/// Patch archives are tiny compared to full releases; keep the same chunk
/// limit as full releases for consistency (well below the 2 GiB asset limit).
const PATCH_CHUNK_SIZE_MB: u64 = 2000;

#[derive(Debug, Clone, Serialize)]
pub struct PatchUploadResult {
  /// Per-repo outcome of tagging the game repositories with the patch tag.
  pub repos: Vec<RepoTagReport>,
  /// Non-fatal issues (e.g. failed tag pushes).
  pub warnings: Vec<String>,
}

fn patch_upload_log(app: &tauri::AppHandle, message: String) {
  let _ = app.emit("patch-upload-log", message);
}

/// Turn the staged faction-editor fragment into the one this patch ships:
/// keep only the props the developer left ticked, and write it under the
/// patch tag so several patches can sit side by side in the player's
/// `appdata/patches`. Returns the props that survived.
///
/// `allowed_fields` is `None` when this session has no selection to apply —
/// the folder was collected in an earlier run of the launcher and picked by
/// hand — and then everything staged travels. `Some(&[])` is the developer
/// explicitly unticking everything, and ships nothing. Conflating the two is
/// what made a re-upload of an already collected folder quietly drop the
/// settings.
///
/// The staged file is left in place: an upload that fails after packing (tag
/// already exists, network) is retried with the same folder, and deleting the
/// staging copy here would make that retry silently ship no settings. It is
/// kept out of the archive by `patch_pack_excludes` instead.
///
/// No staged fragment (the patch changes no settings) — `Ok(vec![])`.
fn finalize_fe_fragment(patch_dir: &Path, tag_name: &str, allowed_fields: Option<&[String]>) -> anyhow::Result<Vec<String>> {
  use crate::consts::{FE_PATCH_FRAGMENT_STAGING, FE_PATCH_FRAGMENT_SUFFIX};
  use crate::service::faction_patch;

  let dir = crate::utils::patch_markers::patches_dir(patch_dir);
  let staged = dir.join(FE_PATCH_FRAGMENT_STAGING);
  if !staged.is_file() {
    return Ok(Vec::new());
  }

  // Re-read through the same validator the player's launcher will use, so an
  // unusable fragment is caught here rather than on a thousand machines.
  let fragment = faction_patch::read_fragment(&staged)?.unwrap_or_default();
  let fragment = match allowed_fields {
    Some(fields) => fragment.retain_fields(fields),
    None => fragment,
  };
  if fragment.is_empty() {
    return Ok(Vec::new());
  }

  crate::utils::patch_markers::assert_safe_patch_name(tag_name)?;
  let final_path = dir.join(faction_patch::fragment_file_name(tag_name));

  // Only now that a replacement is certain: sweeping earlier would also wipe
  // a correct fragment left by a previous upload of this folder whenever this
  // run ends up writing nothing. Reached only when the staging file proves the
  // folder came out of the collector, so pointing the uploader at a real game
  // install cannot sweep the fragments of the patches installed there.
  if let Ok(entries) = fs::read_dir(&dir) {
    for entry in entries.flatten() {
      let name = entry.file_name().to_string_lossy().into_owned();
      if name != FE_PATCH_FRAGMENT_STAGING && name.ends_with(FE_PATCH_FRAGMENT_SUFFIX) {
        fs::remove_file(entry.path()).ok();
      }
    }
  }

  fs::write(&final_path, faction_patch::render_fragment(&fragment))?;

  Ok(fragment.fields)
}

/// What must never end up inside the patch archive: the staging fragment
/// (`finalize_fe_fragment` writes the real one under the patch tag) and the
/// output folder itself, which sits in the very folder being packed — without
/// this, re-packing a patch would bundle the previous archive into the new one.
fn patch_pack_excludes() -> Vec<String> {
  use crate::consts::{APPDATA_DIR, FE_PATCH_FRAGMENT_STAGING, PATCHES_DIR_NAME, PATCH_ARCHIVE_DIR};
  vec![
    format!("{}/{}/{}", APPDATA_DIR, PATCHES_DIR_NAME, FE_PATCH_FRAGMENT_STAGING),
    PATCH_ARCHIVE_DIR.to_string(),
    format!("{}/**", PATCH_ARCHIVE_DIR),
  ]
}

/// Write `sha256.txt` listing every asset of the packed patch, in the format
/// `sha256sum -c` and `certutil -hashfile` output can be checked against.
/// The per-part hashes are already in the manifest; this is for the developer
/// verifying a hand-made re-upload, so it covers `manifest.json` too.
fn write_pack_checksums(pack_dir: &Path, assets: &[ReleaseManifestFile]) -> anyhow::Result<()> {
  let mut out = String::new();
  for asset in assets {
    let path = pack_dir.join(&asset.name);
    if !path.is_file() {
      continue;
    }
    // Reuse the hash the packer already computed; only the manifest needs one.
    let sha = match &asset.sha256 {
      Some(sha) => sha.clone(),
      None => crate::utils::hash::sha256_file(&path, None, None)?,
    };
    out.push_str(&format!("{} *{}\n", sha, asset.name));
  }
  std::fs::write(pack_dir.join(crate::consts::PATCH_ARCHIVE_SHA_FILE), out)?;
  Ok(())
}

/// `true` when the collector left a staging fragment in this folder.
fn fe_fragment_staged(patch_dir: &Path) -> bool {
  crate::utils::patch_markers::patches_dir(patch_dir).join(crate::consts::FE_PATCH_FRAGMENT_STAGING).is_file()
}

/// Streams one patch asset to `asset_url` with `patch-upload-progress` events;
/// returns the number of bytes actually streamed. Re-used by the re-uploads
/// after a server-side hash mismatch (the file is re-opened each attempt).
#[allow(clippy::too_many_arguments)]
async fn upload_patch_asset_stream(
  app: &tauri::AppHandle,
  api: &(dyn crate::providers::ApiProvider::ApiProvider + Send + Sync),
  file_path: &Path,
  asset_name: String,
  asset_url: String,
  total_size: u64,
  uploaded_before: u64,
  grand_total: u64,
  cancel_rx: broadcast::Receiver<()>,
) -> Result<u64, String> {
  let asset_name_for_stream = asset_name.clone();
  let file_handle = File::open(file_path).await.map_err(|e| {
    let err = anyhow::anyhow!(e);
    log_full_error(&err);
    format!("Failed to open file '{}': {}", &asset_name, err)
  })?;
  let file_stream = ReaderStream::new(file_handle);
  let start_time = Instant::now();

  let uploaded_for_emit = Arc::new(std::sync::atomic::AtomicU64::new(0));
  let uploaded_for_emit_in_stream = uploaded_for_emit.clone();
  let mut cancel_rx_for_stream = cancel_rx;
  // Owned handle: the stream is boxed as `dyn Stream + 'static`.
  let app_handle = app.clone();

  let progress_stream = async_stream::stream! {
    let mut uploaded = 0u64;
    for await chunk in file_stream {
      if let Ok(()) = cancel_rx_for_stream.try_recv() {
        log::info!("Patch upload of '{}' cancelled mid-stream", &asset_name_for_stream);
        return;
      }
      if let Ok(ref data) = chunk {
        uploaded += data.len() as u64;
        uploaded_for_emit_in_stream.store(uploaded, std::sync::atomic::Ordering::Relaxed);
        let elapsed = start_time.elapsed().as_secs_f64();
        let speed = if elapsed > 0.0 { uploaded as f64 / elapsed } else { 0.0 };
        let _ = app_handle.emit("patch-upload-progress", UploadProgressPayload {
          file_name: asset_name_for_stream.clone(),
          file_uploaded_size: uploaded,
          file_total_size: total_size,
          total_uploaded_size: uploaded_before + uploaded,
          total_size: grand_total,
          speed,
        });
      }
      yield chunk;
    }
  };
  let boxed_stream: Box<dyn Stream<Item = std::io::Result<Bytes>> + Send + Unpin> = Box::new(Box::pin(progress_stream));

  log::debug!("upload_patch: asset: {} by url: {}", &asset_name, &asset_url);
  api.upload_release_file(&asset_url, total_size, boxed_stream).await.map_err(|e| {
    log_full_error(&e);
    format!("upload_release_file '{}' failed: {}", &asset_name, e)
  })?;

  Ok(uploaded_for_emit.load(std::sync::atomic::Ordering::Relaxed))
}

/// Collects a partial-update patch from the game git repositories:
/// committed changes (latest reachable tag -> HEAD) of every repo found
/// under the selected folder. Heavy git/fs work runs on a blocking thread.
#[tauri::command]
pub async fn collect_patch(source_dir: String, exclude_patterns: Vec<String>) -> Result<patch_collect::PatchCollectResult, String> {
  log::info!("collect_patch: source_dir: {}, exclude_patterns: {}", source_dir, exclude_patterns.len());

  let result =
    tokio::task::spawn_blocking(move || patch_collect::collect_patch(std::path::PathBuf::from(source_dir), exclude_patterns))
      .await
      .map_err(|e| e.to_string())?
      .map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

  log::info!(
    "collect_patch done: repos: {}, changed: {}, deleted: {}",
    result.repos.len(),
    result.changed,
    result.deleted
  );

  Ok(result)
}

/// Cancels an in-progress patch upload by patch tag name.
#[tauri::command]
pub async fn cancel_patch_upload(cancel_map: tauri::State<'_, UploadCancelMap>, patchName: String) -> Result<(), String> {
  let key = format!("patch:{}", patchName);
  if let Some(tx) = crate::utils::locks::lock(&cancel_map).get(&key) {
    let _ = tx.send(());
  }
  Ok(())
}

/// Uploads a patch into the updates repo of a game release.
///
/// Flow: find the updates repo -> detect `base_patch` (latest existing patch
/// release) -> pack the patch folder into split archives with a patch
/// manifest (data*.zip + manifest.json as release assets) -> create tag +
/// release -> upload assets -> tag the game git repositories with the patch
/// tag (anchors the diff base for the next patch).
///
/// No resume: patches are small, and the resume infrastructure of upload_v2
/// is bound to the single `progress_upload` slot in the config.
#[tauri::command]
pub async fn upload_patch(
  app: tauri::AppHandle,
  cancel_map: tauri::State<'_, UploadCancelMap>,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  name: String,
  patchName: String,
  patchDir: String,
  gameSourceDir: Option<String>,
  deletedFiles: Vec<String>,
  baseReleaseTag: Option<String>,
  // `None` — the frontend has no selection for this folder (collected in an
  // earlier session); everything staged then travels.
  updatedFields: Option<Vec<String>>,
) -> Result<PatchUploadResult, String> {
  let patch_name_raw = patchName.trim().to_string();
  if patch_name_raw.is_empty() {
    return Err("Patch name must not be empty".to_string());
  }
  let tag_name = make_tag_name(&patch_name_raw);
  if !Path::new(&patchDir).is_dir() {
    return Err(format!("Patch dir does not exist: {}", patchDir));
  }

  // Cancel map guard (keyed distinctly from full-release uploads).
  let cancel_key = format!("patch:{}", tag_name);
  if crate::utils::locks::lock(&cancel_map).contains_key(&cancel_key) {
    return Err("PATCH_UPLOAD_ALREADY_RUNNING".to_string());
  }
  let (cancel_tx, _) = broadcast::channel::<()>(1);
  crate::utils::locks::lock(&cancel_map).insert(cancel_key.clone(), cancel_tx.clone());
  scopeguard::defer! { crate::utils::locks::lock(&cancel_map).remove(&cancel_key); };

  // Get api_client (drop Service guard immediately, mirrors upload_v2).
  let api_client = {
    let service_guard = service.lock().await;
    service_guard.api_client.clone()
  };
  let api = api_client.current_provider().map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  patch_upload_log(&app, format!("Uploading patch '{}' for release '{}' ...", &tag_name, &name));

  // ------------------------------------------------------------------
  // 1. Find the updates repo of the release.
  // ------------------------------------------------------------------
  let updates_project = resolve_updates_project(&api_client, &name)
    .await
    .map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;
  let project_id = project_id_for(&api_client, &updates_project).map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  patch_upload_log(&app, format!("Updates repo: {}", &project_id));

  // ------------------------------------------------------------------
  // 2. Detect base_patch = latest existing patch release in the chain.
  // ------------------------------------------------------------------
  let mut repo_releases = api.get_repo_releases(&project_id).await.map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;
  // Newest first (None sorts last).
  repo_releases.sort_by(|a, b| b.created_at.cmp(&a.created_at));
  let base_patch = repo_releases.first().map(|r| r.tag_name.clone());
  let already_exists = repo_releases.iter().any(|r| r.tag_name == tag_name);
  if let Some(bp) = &base_patch {
    patch_upload_log(&app, format!("Base patch: {}", bp));
  } else {
    patch_upload_log(&app, "First patch after full release".to_string());
  }

  // Ensure the updates repo has at least one commit so that
  // create_tag (GitLab, which needs ref=master) and create_release
  // (GitHub, which needs target_commitish) work on freshly created
  // empty repos.
  if base_patch.is_none() {
    patch_upload_log(&app, "Initializing empty updates repo ...".to_string());
    let _ = api
      .add_file_to_repo(&project_id, ".gitkeep", "", "Initialize updates repo", DEFAULT_BRANCH)
      .await;
  }

  // ------------------------------------------------------------------
  // 3. Pack the patch folder into split archives + patch manifest.
  // ------------------------------------------------------------------
  // Next to the patch, not in %TEMP%: a failed or partial upload then leaves
  // a ready archive the developer can re-upload by hand.
  let pack_dir = Path::new(&patchDir).join(crate::consts::PATCH_ARCHIVE_DIR);
  let pack_dir_str = pack_dir.to_string_lossy().into_owned();
  // Name the faction-editor fragment after the patch and drop the props the
  // developer unticked. The collector could not do this: the patch tag only
  // exists here. Failure is not fatal — the patch itself still ships.
  let fe_updated_fields = match finalize_fe_fragment(Path::new(&patchDir), &tag_name, updatedFields.as_deref()) {
    Ok(fields) => {
      if !fields.is_empty() {
        patch_upload_log(&app, format!("Faction editor settings in this patch: {}", fields.join(", ")));
      } else if fe_fragment_staged(Path::new(&patchDir)) {
        // Only reachable now when the developer unticked every prop, since an
        // absent selection ships everything. Still worth saying out loud.
        patch_upload_log(
          &app,
          "A faction editor settings fragment is staged in this folder, but every prop was unticked — the patch ships without settings.".to_string(),
        );
      }
      fields
    }
    Err(e) => {
      log::warn!("upload_patch: faction editor fragment skipped: {}", e);
      patch_upload_log(&app, format!("Faction editor settings were skipped: {}", e));
      Vec::new()
    }
  };

  // Source of truth for the save-breaking flag: re-scan the actual folder and
  // the delete list. `upload_patch` receives a folder the collector may have
  // built in an earlier session (or that was touched by hand), so the collect
  // result cannot be trusted here. The walk is cheap — the packer reads the
  // same files right after.
  let save_breaking_files = patch_collect::scan_save_breaking(Path::new(&patchDir), &deletedFiles);
  let breaks_saves = !save_breaking_files.is_empty();
  if breaks_saves {
    let shown = save_breaking_files.iter().take(20).cloned().collect::<Vec<_>>().join(", ");
    patch_upload_log(
      &app,
      format!(
        "WARNING: this patch breaks existing save games, marker files ({}): {}",
        save_breaking_files.len(),
        shown
      ),
    );
  }

  let patch_meta = PatchMeta {
    patch_name: tag_name.clone(),
    base_patch: base_patch.clone(),
    base_release_tag: baseReleaseTag.clone().filter(|s| !s.is_empty()),
    deleted_files: deletedFiles.clone(),
    updated_fields: fe_updated_fields,
    breaks_saves,
  };

  patch_upload_log(&app, "Packing patch archives ...".to_string());
  let mut manifest = pack_split_archives(
    &app,
    patchDir.clone(),
    pack_dir_str.clone(),
    PATCH_CHUNK_SIZE_MB,
    patch_pack_excludes(),
    None,
    Some(patch_meta),
    Vec::new(),
  )
  .await?;

  // The patch manifest itself is uploaded as a release asset (NOT committed
  // into the repo: full releases already own the single manifest.json path).
  // kind = Manifest so installers skip it as data; sha256 stays None.
  let manifest_size = fs::metadata(pack_dir.join(MANIFEST_NAME)).map(|m| m.len()).unwrap_or(0);
  manifest.files.push(ReleaseManifestFile {
    name: MANIFEST_NAME.to_string(),
    size: manifest_size,
    sha256: None,
    kind: crate::handlers::dto::ManifestFileKind::Manifest,
    target: None,
  });

  if let Err(e) = write_pack_checksums(&pack_dir, &manifest.files) {
    // Only a convenience for a manual re-upload; never worth failing on.
    log::warn!("cannot write patch checksums: {}", e);
  }
  patch_upload_log(&app, format!("Patch archive: {}", pack_dir_str));

  // ------------------------------------------------------------------
  // 4. Create tag + release, then upload every asset.
  // ------------------------------------------------------------------
  if already_exists {
    patch_upload_log(&app, format!("Tag '{}' already exists (retry after interrupted upload), skipping tag creation", &tag_name));
  } else {
    patch_upload_log(&app, format!("Creating tag '{}' in updates repo ...", &tag_name));
    if let Err(e) = api.create_tag(&project_id, &tag_name, DEFAULT_BRANCH).await {
      patch_upload_log(&app, format!("Warning: create_tag '{}' failed (may already exist): {}", &tag_name, e));
    }
  }

  let first_assets: Vec<CreateReleaseAsset> = manifest
    .files
    .iter()
    .map(|file| {
      let url = crate::handlers::upload_v2::build_asset_url(&api.get_asset_url(), &project_id, "gw_releases", &tag_name, &file.name);
      CreateReleaseAsset {
        file_name: file.name.clone(),
        file_download_url: url,
      }
    })
    .collect();

  patch_upload_log(&app, format!("Creating release '{}' ...", &tag_name));
  let created_release = match api.create_release(&project_id, &tag_name, first_assets).await {
    Ok(r) => r,
    Err(e) => {
      if already_exists {
        return Err(format!(
          "Release '{}' already exists from a previous interrupted upload. \
           Delete the release and tag '{}' manually in the updates repo, then retry. \
           Original error: {}",
          &tag_name, &tag_name, e
        ));
      }
      return Err(format!("create_release '{}' failed: {}", &tag_name, e));
    }
  };
  let upload_template = created_release.upload_url;

  let grand_total: u64 = manifest.files.iter().map(|f| f.size).sum();
  let total_count = manifest.files.len() as u32;
  let mut done_count: u32 = 0;
  let mut uploaded_before: u64 = 0;
  let _ = app.emit("patch-upload-files-count", (done_count, total_count));

  for file in &manifest.files {
    let asset_url = build_asset_url(&upload_template, &project_id, "gw_releases", &tag_name, &file.name);
    let asset_name = file.name.clone();
    let total_size = file.size;
    let file_path = pack_dir.join(&asset_name);

    // Upload + server-side hash verification loop: on a mismatch the remote
    // asset is deleted and re-uploaded, up to MAX_UPLOAD_VERIFY_RETRIES times.
    let mut verify_attempts: u32 = 0;
    loop {
      // Cancel check before each attempt.
      if cancel_tx.receiver_count() > 0 {
        let mut probe = cancel_tx.subscribe();
        if probe.try_recv().is_ok() {
          patch_upload_log(&app, format!("Patch upload cancelled before file: {}", &file.name));
          return Err("USER_CANCELLED".to_string());
        }
      }

      let actually_uploaded = upload_patch_asset_stream(
        &app,
        api,
        &file_path,
        asset_name.clone(),
        asset_url.clone(),
        total_size,
        uploaded_before,
        grand_total,
        cancel_tx.subscribe(),
      )
      .await?;

      if actually_uploaded < total_size {
        patch_upload_log(&app, format!("Upload of '{}' was interrupted ({} of {} bytes)", &asset_name, actually_uploaded, total_size));
        return Err("USER_CANCELLED".to_string());
      }

      let Some(expected) = file.sha256.as_deref().filter(|s| !s.is_empty()) else {
        break;
      };

      match api.get_uploaded_asset_sha256(&project_id, &tag_name, &asset_name).await {
        Ok(Some(remote)) if remote.eq_ignore_ascii_case(expected) => {
          patch_upload_log(&app, format!("File {}: sha256 verified on server", &asset_name));
          break;
        }
        Ok(Some(remote)) => {
          verify_attempts += 1;
          if verify_attempts > crate::consts::MAX_UPLOAD_VERIFY_RETRIES {
            patch_upload_log(&app, format!("File {}: server sha256 {} != local {} after {} attempts", &asset_name, &remote, expected, crate::consts::MAX_UPLOAD_VERIFY_RETRIES));
            return Err(crate::consts::ERR_UPLOAD_HASH_MISMATCH.to_string());
          }
          patch_upload_log(&app, format!("File {}: server sha256 {} != local {}, deleting asset and re-uploading (attempt {}/{})", &asset_name, &remote, expected, verify_attempts, crate::consts::MAX_UPLOAD_VERIFY_RETRIES));
          if let Err(e) = api.delete_release_asset(&project_id, &tag_name, &asset_name).await {
            // Fatal: see the identical comment in upload_v2.rs — GitHub
            // rejects a re-upload under an existing asset name, so a failed
            // delete must not be swallowed as a warning.
            patch_upload_log(&app, format!("File {}: failed to delete stale asset before re-upload: {}", &asset_name, e));
            return Err(crate::consts::ERR_UPLOAD_HASH_MISMATCH.to_string());
          }
        }
        Ok(None) => {
          patch_upload_log(&app, format!("File {}: server returned no sha256, verification skipped", &asset_name));
          break;
        }
        Err(e) => {
          log::warn!("Cannot fetch server sha256 of '{}': {} — verification skipped", &asset_name, e);
          break;
        }
      }
    }

    uploaded_before += total_size;
    done_count += 1;
    let _ = app.emit("patch-upload-files-count", (done_count, total_count));
    patch_upload_log(&app, format!("File {} uploaded successful !", &asset_name));
  }

  patch_upload_log(&app, format!("Patch '{}' uploaded successful !", &tag_name));

  // Best-effort: re-publish the static release index.  Non-fatal but visible.
  if let Err(e) = crate::service::index_publisher::publish_index(api, false).await {
    log::warn!("Failed to publish release index after patch upload: {}", e);
    patch_upload_log(&app, format!("WARNING: Failed to publish release index: {}. The patch may not appear for players until the index is re-published manually.", e));
  }

  // Invalidate AFTER publishing (see the same note in upload_v2.rs).
  {
    let mut svc = service.lock().await;
    svc.invalidate_releases();
  }

  // ------------------------------------------------------------------
  // 5. Tag the game git repositories with the patch tag (anchors the
  //    diff base for the NEXT patch). Never aborts the finished upload.
  // ------------------------------------------------------------------
  let mut warnings: Vec<String> = Vec::new();
  let repos: Vec<RepoTagReport> = match gameSourceDir.as_deref().filter(|s| !s.is_empty()) {
    Some(source) => {
      patch_upload_log(&app, format!("Tagging game repositories with '{}' ...", &tag_name));
      let source = source.to_string();
      let tag = tag_name.clone();
      tokio::task::spawn_blocking(move || patch_collect::tag_game_repos(Path::new(&source), &tag))
        .await
        .map_err(|e| e.to_string())?
    }
    None => {
      warnings.push("Game source dir not provided: game repositories were not tagged. Next patch may collect duplicates.".to_string());
      Vec::new()
    }
  };
  for repo in &repos {
    if !repo.pushed {
      warnings.push(format!("repo '{}': {}", repo.repo_rel_path, repo.message.clone().unwrap_or_else(|| "not pushed".to_string())));
    }
  }

  // The pack dir is deliberately NOT removed: it is the archive the developer
  // re-uploads by hand when a release ends up half-published. The next pack of
  // this folder cleans it (`cleanup_previous_pack`).
  patch_upload_log(&app, format!("Archive kept for manual re-upload: {}", pack_dir_str));

  log::info!("upload_patch done: release: {} patch: {} repos tagged: {}", &name, &tag_name, repos.len());
  let _ = app.emit("patch-upload-files-count", (total_count, total_count));

  Ok(PatchUploadResult { repos, warnings })
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The staging fragment is kept out of the archive by an exclude pattern
  /// spelled with `/`, while `pack_split_archives` matches it against paths
  /// WalkDir yields with the platform separator. globset normalizes `\` on
  /// Windows — this pins that down, because a silent miss would ship
  /// `_pending.faction_editor_patch.ltx` into every player's appdata/patches.
  fn staged_dir(name: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("gw_finalize_{}_{}_{}", name, std::process::id(), n));
    let _ = fs::remove_dir_all(&root);
    let dir = crate::utils::patch_markers::patches_dir(&root);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
      dir.join(crate::consts::FE_PATCH_FRAGMENT_STAGING),
      concat!(
        "[alfa]
        power = 15
        spot_color_r = 200
",
        "
[alfa_veteran]
        fire_wound_immunity = 0.36
",
      ),
    )
    .unwrap();
    root
  }

  fn final_fragment(root: &std::path::Path, tag: &str) -> Option<String> {
    let p = crate::utils::patch_markers::patches_dir(root).join(crate::service::faction_patch::fragment_file_name(tag));
    fs::read_to_string(p).ok()
  }

  /// The owner hit this live: a folder collected in an earlier launcher run and
  /// then uploaded again arrives with no field list. Treating that as "nothing
  /// ticked" shipped a patch without its settings — and, because the sweep ran
  /// first, also deleted the correct fragment the previous upload had written.
  #[test]
  fn absent_selection_ships_everything_staged() {
    let root = staged_dir("none");
    let fields = finalize_fe_fragment(&root, "0.5.6-patch1", None).unwrap();

    assert_eq!(fields, vec!["fire_wound_immunity", "power", "spot_color_r"]);
    let written = final_fragment(&root, "0.5.6-patch1").expect("fragment must be written");
    assert!(written.contains("power                            = 15"));
    assert!(written.contains("fire_wound_immunity              = 0.36"));
    // Staging survives so a retry after a failed upload still has the data.
    assert!(fe_fragment_staged(&root));

    fs::remove_dir_all(&root).ok();
  }

  #[test]
  fn explicit_selection_filters_and_empty_selection_ships_nothing() {
    let root = staged_dir("some");
    let fields = finalize_fe_fragment(&root, "0.5.6-patch1", Some(&["power".to_string()])).unwrap();
    assert_eq!(fields, vec!["power"]);
    let written = final_fragment(&root, "0.5.6-patch1").unwrap();
    assert!(written.contains("power"));
    assert!(!written.contains("fire_wound_immunity"));

    let root2 = staged_dir("empty");
    assert!(finalize_fe_fragment(&root2, "0.5.6-patch1", Some(&[])).unwrap().is_empty());
    assert!(final_fragment(&root2, "0.5.6-patch1").is_none(), "nothing ticked ships no fragment");

    fs::remove_dir_all(&root).ok();
    fs::remove_dir_all(&root2).ok();
  }

  /// Re-uploading the same folder must end with exactly one fragment, and a run
  /// that writes nothing must not destroy the previous one.
  #[test]
  fn resupload_replaces_one_fragment_and_never_orphans_the_previous() {
    let root = staged_dir("resupload");
    finalize_fe_fragment(&root, "0.5.6-patch1", None).unwrap();
    // A second upload under a different tag: one fragment, the new one.
    finalize_fe_fragment(&root, "0.5.6-patch2", None).unwrap();
    assert!(final_fragment(&root, "0.5.6-patch1").is_none(), "the stale tag must be swept");
    assert!(final_fragment(&root, "0.5.6-patch2").is_some());

    // A run that ships nothing leaves the existing fragment alone.
    finalize_fe_fragment(&root, "0.5.6-patch3", Some(&[])).unwrap();
    assert!(final_fragment(&root, "0.5.6-patch2").is_some(), "a no-op upload must not delete the good fragment");

    fs::remove_dir_all(&root).ok();
  }

  /// The excludes are spelled with `/`, while `pack_split_archives` matches
  /// them against paths WalkDir yields with the platform separator. globset
  /// normalizes the separator on Windows — this pins that down, because a
  /// silent miss would ship the staging fragment into every player's
  /// appdata/patches, or bundle the previous archive into the next one.
  #[test]
  fn pack_excludes_match_platform_paths() {
    use globset::{GlobBuilder, GlobSetBuilder};
    use std::path::Path;

    let mut builder = GlobSetBuilder::new();
    for pattern in patch_pack_excludes() {
      builder.add(GlobBuilder::new(&pattern).case_insensitive(true).build().unwrap());
    }
    let set = builder.build().unwrap();

    for excluded in [
      "appdata/patches/_pending.faction_editor_patch.ltx",
      "_archive",
      "_archive/data1.zip",
      "_archive/manifest.json",
      "_archive/sha256.txt",
    ] {
      assert!(set.is_match(Path::new(excluded)), "{} must be excluded", excluded);
    }
    if cfg!(windows) {
      assert!(set.is_match(Path::new("appdata\\patches\\_pending.faction_editor_patch.ltx")));
      assert!(set.is_match(Path::new("_archive\\data1.zip")));
    }

    for shipped in [
      "appdata/patches/0.5.6-patch1.faction_editor_patch.ltx",
      "appdata/patches/0.5.6-patch1.json",
      "gamedata/configs/faction_editor_default_config.ltx",
    ] {
      assert!(!set.is_match(Path::new(shipped)), "{} must travel", shipped);
    }
  }
}
