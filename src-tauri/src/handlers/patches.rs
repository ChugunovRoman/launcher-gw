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
use uuid::Uuid;

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
  if let Some(tx) = cancel_map.lock().unwrap().get(&key) {
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
  if cancel_map.lock().unwrap().contains_key(&cancel_key) {
    return Err("PATCH_UPLOAD_ALREADY_RUNNING".to_string());
  }
  let (cancel_tx, _) = broadcast::channel::<()>(1);
  cancel_map.lock().unwrap().insert(cancel_key.clone(), cancel_tx.clone());
  scopeguard::defer! { cancel_map.lock().unwrap().remove(&cancel_key); };

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
  let pack_dir = std::env::temp_dir().join(format!("gw-patch-pack-{}", Uuid::new_v4()));
  let pack_dir_str = pack_dir.to_string_lossy().into_owned();
  let patch_meta = PatchMeta {
    patch_name: tag_name.clone(),
    base_patch: base_patch.clone(),
    base_release_tag: baseReleaseTag.clone().filter(|s| !s.is_empty()),
    deleted_files: deletedFiles.clone(),
  };

  patch_upload_log(&app, "Packing patch archives ...".to_string());
  let mut manifest = pack_split_archives(
    &app,
    patchDir.clone(),
    pack_dir_str.clone(),
    PATCH_CHUNK_SIZE_MB,
    vec![],
    None,
    Some(patch_meta),
  )
  .await?;

  // The patch manifest itself is uploaded as a release asset (NOT committed
  // into the repo: full releases already own the single manifest.json path).
  let manifest_size = fs::metadata(pack_dir.join(MANIFEST_NAME)).map(|m| m.len()).unwrap_or(0);
  manifest.files.push(ReleaseManifestFile {
    name: MANIFEST_NAME.to_string(),
    size: manifest_size,
  });

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
      let url = api
        .get_asset_url()
        .replace("<PROJECT_ID>", &project_id)
        .replace("<NAME_SPACE>", "gw_releases")
        .replace("<VERSION>", &tag_name)
        .replace("<FILE_NAME>", &file.name);
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
    // Cancel check before opening.
    if cancel_tx.receiver_count() > 0 {
      let mut probe = cancel_tx.subscribe();
      if probe.try_recv().is_ok() {
        patch_upload_log(&app, format!("Patch upload cancelled before file: {}", &file.name));
        return Err("USER_CANCELLED".to_string());
      }
    }

    let asset_url = build_asset_url(&upload_template, &project_id, &tag_name, &file.name);
    let asset_name = file.name.clone();
    let asset_name_for_stream = asset_name.clone();
    let total_size = file.size;
    let file_handle = File::open(pack_dir.join(&asset_name)).await.map_err(|e| {
      let err = anyhow::anyhow!(e);
      log_full_error(&err);
      format!("Failed to open file '{}': {}", &asset_name, err)
    })?;
    let file_stream = ReaderStream::new(file_handle);
    let start_time = Instant::now();

    let uploaded_before_this_file = uploaded_before;
    let uploaded_for_emit = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let uploaded_for_emit_in_stream = uploaded_for_emit.clone();
    let mut cancel_rx_for_stream = cancel_tx.subscribe();
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
            total_uploaded_size: uploaded_before_this_file + uploaded,
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

    let actually_uploaded = uploaded_for_emit.load(std::sync::atomic::Ordering::Relaxed);
    if actually_uploaded < total_size {
      patch_upload_log(&app, format!("Upload of '{}' was interrupted ({} of {} bytes)", &asset_name, actually_uploaded, total_size));
      return Err("USER_CANCELLED".to_string());
    }

    uploaded_before += total_size;
    done_count += 1;
    let _ = app.emit("patch-upload-files-count", (done_count, total_count));
    patch_upload_log(&app, format!("File {} uploaded successful !", &asset_name));
  }

  patch_upload_log(&app, format!("Patch '{}' uploaded successful !", &tag_name));

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

  // Best-effort cleanup of the temp pack dir.
  if let Err(e) = fs::remove_dir_all(&pack_dir) {
    log::warn!("cannot remove patch pack dir {:?}: {}", pack_dir, e);
  }

  log::info!("upload_patch done: release: {} patch: {} repos tagged: {}", &name, &tag_name, repos.len());
  let _ = app.emit("patch-upload-files-count", (total_count, total_count));

  Ok(PatchUploadResult { repos, warnings })
}
