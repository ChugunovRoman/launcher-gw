use bytes::Bytes;
use futures_util::Stream;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;
use std::{collections::HashMap, fs};
use tauri::{Emitter, Manager};
use tokio::{fs::File, sync::Mutex, sync::broadcast};
use tokio_util::io::ReaderStream;

use crate::{
  configs::AppConfig::{AppConfig, VersionProgressUpload},
  consts::{DEFAULT_BRANCH, MANIFEST_NAME},
  handlers::dto::{ReleaseManifest, UploadProgressPayload},
  providers::dto::CreateReleaseAsset,
  service::get_release::ServiceGetRelease,
  service::main::Service,
  utils::errors::{log_full_error, upload_log},
};

/// Separate cancel map for uploads (keyed by release name). Kept distinct from the
/// download cancel map so cancelling one cannot accidentally hit the other.
pub type UploadCancelMap = Arc<StdMutex<HashMap<String, broadcast::Sender<()>>>>;

const NAMESPACE: &str = "gw_releases";

/// Cancels an in-progress upload by release name.
#[tauri::command]
pub async fn cancel_upload(cancel_map: tauri::State<'_, UploadCancelMap>, name: String) -> Result<(), String> {
  if let Some(tx) = cancel_map.lock().unwrap().get(&name) {
    let _ = tx.send(());
  }
  Ok(())
}

/// Shared context for all upload steps. Built once, passed to each step helper.
struct UploadContext {
  app: tauri::AppHandle,
  app_config: Arc<Mutex<AppConfig>>,
  cancel_tx: broadcast::Sender<()>,
  name: String,
  base_dir: PathBuf,
  manifest_content: String,
  manifest_release: ReleaseManifest,
  tag_name: String,
  project_id: String,
  /// Asset URLs filled after `step_create_release` (template with <FILE_NAME>).
  upload_url: Arc<Mutex<String>>,
}

/// Compute the tag name from a release name (whitespace → dashes).
pub(crate) fn make_tag_name(name: &str) -> String {
  Regex::new(r"\s+").unwrap().replace_all(name, "-").to_string()
}

/// Build asset URLs for a file from a template `upload_url` (with <FILE_NAME>).
pub(crate) fn build_asset_url(upload_template: &str, project_id: &str, tag_name: &str, file_name: &str) -> String {
  upload_template
    .replace("<PROJECT_ID>", project_id)
    .replace("<NAME_SPACE>", NAMESPACE)
    .replace("<VERSION>", tag_name)
    .replace("<FILE_NAME>", file_name)
}

/// ------------------------------------------------------------------
/// Step 1: upload manifest.json into the repo.
/// ------------------------------------------------------------------
async fn step_manifest_upload(ctx: &UploadContext, api: &(dyn crate::providers::ApiProvider::ApiProvider + Send + Sync)) -> Result<(), String> {
  upload_log(&ctx.app, format!("Uploading {} ...", MANIFEST_NAME));
  api
    .add_file_to_repo(&ctx.project_id, MANIFEST_NAME, &ctx.manifest_content, "Upload manifest.json", DEFAULT_BRANCH)
    .await
    .map_err(|e| {
      log_full_error(&e);
      format!("add_file_to_repo (manifest) failed: {}", e)
    })?;

  // Persist progress.
  {
    let mut cfg = ctx.app_config.lock().await;
    if let Some(ref mut p) = cfg.progress_upload {
      p.manifest_uploaded = true;
    }
    let _ = cfg.save();
  }
  upload_log(&ctx.app, format!("File {} upload successful !", MANIFEST_NAME));
  Ok(())
}

/// ------------------------------------------------------------------
/// Step 2: create git tag.
/// ------------------------------------------------------------------
async fn step_create_tag(ctx: &UploadContext, api: &(dyn crate::providers::ApiProvider::ApiProvider + Send + Sync)) -> Result<(), String> {
  upload_log(&ctx.app, format!("Creating tag '{}' ...", &ctx.tag_name));
  api.create_tag(&ctx.project_id, &ctx.tag_name, DEFAULT_BRANCH).await.map_err(|e| {
    log_full_error(&e);
    format!("create_tag '{}' failed: {}", &ctx.tag_name, e)
  })?;

  {
    let mut cfg = ctx.app_config.lock().await;
    if let Some(ref mut p) = cfg.progress_upload {
      p.tag_created = true;
    }
    let _ = cfg.save();
  }
  upload_log(&ctx.app, format!("Tag {} created successful !", &ctx.tag_name));
  Ok(())
}

/// ------------------------------------------------------------------
/// Step 3: create release on provider, store upload_url template.
/// ------------------------------------------------------------------
async fn step_create_release(ctx: &UploadContext, api: &(dyn crate::providers::ApiProvider::ApiProvider + Send + Sync)) -> Result<(), String> {
  // First-pass asset URLs (some providers use them to register links).
  let first_assets: Vec<CreateReleaseAsset> = ctx
    .manifest_release
    .files
    .iter()
    .map(|file| {
      let url = api.get_asset_url().replace("<PROJECT_ID>", &ctx.project_id).replace("<NAME_SPACE>", NAMESPACE).replace("<VERSION>", &ctx.tag_name).replace("<FILE_NAME>", &file.name);
      CreateReleaseAsset { file_name: file.name.clone(), file_download_url: url }
    })
    .collect();

  upload_log(&ctx.app, format!("Creating release for tag '{}' ...", &ctx.tag_name));
  let created_release = api.create_release(&ctx.project_id, &ctx.tag_name, first_assets).await.map_err(|e| {
    log_full_error(&e);
    format!("create_release '{}' failed: {}", &ctx.tag_name, e)
  })?;

  // Persist upload_url template + total_files so resume can rebuild asset URLs.
  {
    let mut cfg = ctx.app_config.lock().await;
    if let Some(ref mut p) = cfg.progress_upload {
      p.release_created = true;
      p.upload_url = created_release.upload_url.clone();
      p.total_files = ctx.manifest_release.files.len() as u32;
    }
    let _ = cfg.save();
  }
  *ctx.upload_url.lock().await = created_release.upload_url;

  upload_log(&ctx.app, format!("Release for {} created successful !", &ctx.tag_name));
  Ok(())
}

/// ------------------------------------------------------------------
/// Step 4: upload each asset file (skip already-uploaded in resume mode).
/// ------------------------------------------------------------------
async fn step_upload_assets(ctx: &UploadContext, api: &(dyn crate::providers::ApiProvider::ApiProvider + Send + Sync)) -> Result<(), String> {
  // Recover the upload template (either from config or from context).
  let upload_template = {
    let cfg = ctx.app_config.lock().await;
    cfg.progress_upload.as_ref().map(|p| p.upload_url.clone()).unwrap_or_default()
  };
  if !upload_template.is_empty() {
    *ctx.upload_url.lock().await = upload_template.clone();
  }
  let upload_template = ctx.upload_url.lock().await.clone();

  // Set of already-uploaded filenames (for resume). Deduplicate the stored Vec
  // defensively — previous concurrent runs may have left duplicate entries.
  let already_uploaded: std::collections::HashSet<String> = {
    let mut cfg = ctx.app_config.lock().await;
    if let Some(ref mut p) = cfg.progress_upload {
      let original_len = p.uploaded_files.len();
      let mut seen: std::collections::HashSet<String> = p.uploaded_files.iter().cloned().collect();
      if seen.len() != original_len {
        // Vec had duplicates — rewrite with deduplicated values and persist.
        let dedup: Vec<String> = seen.drain().collect();
        log::warn!("Detected {} duplicate(s) in uploaded_files, deduplicated", original_len - dedup.len());
        p.uploaded_files = dedup;
        let _ = cfg.save();
      }
      cfg.progress_upload.as_ref().map(|p| p.uploaded_files.iter().cloned().collect()).unwrap_or_default()
    } else {
      std::collections::HashSet::new()
    }
  };

  // Grand total across all files (for progress %).
  let grand_total: u64 = ctx
    .manifest_release
    .files
    .iter()
    .map(|f| fs::metadata(ctx.base_dir.join(&f.name)).map(|m| m.len()).unwrap_or(0))
    .sum();

  // Running bytes uploaded BEFORE the current file (across already-uploaded files
  // AND files uploaded earlier in this session). Incremented as we go.
  let mut uploaded_before: u64 = 0;

  // Counter of fully-uploaded files (already-done from resume + new ones this session).
  // Emitted via `upload-files-count` so the UI's `files=N/total` counter updates live.
  let total_count = ctx.manifest_release.files.len() as u32;
  let mut done_count = already_uploaded.len() as u32;
  let _ = ctx.app.emit("upload-files-count", (done_count, total_count));

  for file in &ctx.manifest_release.files {
    let file_size = fs::metadata(ctx.base_dir.join(&file.name)).map(|m| m.len()).unwrap_or(0);

    // Resume: skip files already uploaded, but emit a 100% progress event so the
    // UI shows their progress bar as complete (matching the start-from-scratch view).
    if already_uploaded.contains(&file.name) {
      log::info!("Skipping already-uploaded file: {}", &file.name);
      let _ = ctx.app.emit("upload-progress", UploadProgressPayload {
        file_name: file.name.clone(),
        file_uploaded_size: file_size,
        file_total_size: file_size,
        total_uploaded_size: uploaded_before + file_size,
        total_size: grand_total,
        speed: 0.0,
      });
      uploaded_before += file_size;
      continue;
    }

    // Cancel check before opening.
    if ctx.cancel_tx.receiver_count() > 0 {
      let mut probe = ctx.cancel_tx.subscribe();
      if probe.try_recv().is_ok() {
        upload_log(&ctx.app, format!("Upload cancelled before file: {}", &file.name));
        return Err("USER_CANCELLED".to_string());
      }
    }

    let asset_url = build_asset_url(&upload_template, &ctx.project_id, &ctx.tag_name, &file.name);
    let asset_name = file.name.clone();
    let asset_name_for_log = asset_name.clone();
    let asset_name_for_stream = asset_name.clone();
    let app_handle = ctx.app.clone();

    let file_handle = File::open(ctx.base_dir.join(&asset_name)).await.map_err(|e| {
      let err = anyhow::anyhow!(e);
      log_full_error(&err);
      format!("Failed to open file '{}': {}", &asset_name, err)
    })?;
    let total_size = file_size;
    let file_stream = ReaderStream::new(file_handle);
    let start_time = Instant::now();

    let uploaded_before_this_file = uploaded_before;
    let uploaded_for_emit = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let uploaded_for_emit_in_stream = uploaded_for_emit.clone();
    let mut cancel_rx_for_stream = ctx.cancel_tx.subscribe();
    let grand_total_for_stream = grand_total;

    let progress_stream = async_stream::stream! {
      let mut uploaded = 0u64;
      for await chunk in file_stream {
        if let Ok(()) = cancel_rx_for_stream.try_recv() {
          log::info!("Upload of '{}' cancelled mid-stream", &asset_name_for_stream);
          return;
        }
        if let Ok(ref data) = chunk {
          uploaded += data.len() as u64;
          uploaded_for_emit_in_stream.store(uploaded, std::sync::atomic::Ordering::Relaxed);
          let elapsed = start_time.elapsed().as_secs_f64();
          let speed = if elapsed > 0.0 { uploaded as f64 / elapsed } else { 0.0 };
          let _ = app_handle.emit("upload-progress", UploadProgressPayload {
            file_name: asset_name_for_stream.clone(),
            file_uploaded_size: uploaded,
            file_total_size: total_size,
            total_uploaded_size: uploaded_before_this_file + uploaded,
            total_size: grand_total_for_stream,
            speed,
          });
        }
        yield chunk;
      }
    };

    let boxed_stream: Box<dyn Stream<Item = std::io::Result<Bytes>> + Send + Unpin> = Box::new(Box::pin(progress_stream));

    log::debug!("Try upload asset: {} by url: {}", &asset_name_for_log, &asset_url);
    api.upload_release_file(&asset_url, total_size, boxed_stream).await.map_err(|e| {
      log_full_error(&e);
      format!("upload_release_file '{}' failed: {}", &asset_name_for_log, e)
    })?;

    let actually_uploaded = uploaded_for_emit.load(std::sync::atomic::Ordering::Relaxed);
    if actually_uploaded < total_size {
      upload_log(&ctx.app, format!("Upload of '{}' was interrupted ({} of {} bytes)", &asset_name, actually_uploaded, total_size));
      return Err("USER_CANCELLED".to_string());
    }

    uploaded_before += total_size;

    // Persist uploaded file name into config (guard against duplicates defensively).
    {
      let mut cfg = ctx.app_config.lock().await;
      if let Some(ref mut p) = cfg.progress_upload {
        if !p.uploaded_files.iter().any(|n| n == &asset_name) {
          p.uploaded_files.push(asset_name.clone());
        }
      }
      let _ = cfg.save();
    }

    // Emit updated files counter so the UI shows `files=N/total` in real time.
    done_count += 1;
    let _ = ctx.app.emit("upload-files-count", (done_count, total_count));

    upload_log(&ctx.app, format!("File {} uploaded successful !", &asset_name));
  }

  Ok(())
}

/// ------------------------------------------------------------------
/// Step 5: finalize — set release visibility, clear progress.
/// ------------------------------------------------------------------
async fn step_finalize(ctx: &UploadContext, api: &(dyn crate::providers::ApiProvider::ApiProvider + Send + Sync), release_id: String) -> Result<(), String> {
  api.set_release_visibility(&release_id, true).await.map_err(|e| {
    log_full_error(&e);
    format!("set_release_visibility '{}' failed: {}", &release_id, e)
  })?;

  // Mark completed and clear progress_upload.
  {
    let mut cfg = ctx.app_config.lock().await;
    cfg.progress_upload = None;
    let _ = cfg.save();
  }
  upload_log(&ctx.app, "FULL Upload completed successful !".to_string());
  log::info!("Full upload of version {} finish successful !", &ctx.name);

  // Best-effort: re-publish the static release index so players see the
  // new release without hitting the API.  Errors are non-fatal.
  if let Err(e) = crate::service::index_publisher::publish_index(api).await {
    log::warn!("Failed to publish release index after upload: {}", e);
  }

  Ok(())
}

// ==================================================================
// Command 1: upload_v2_release — start from scratch.
// ==================================================================
#[tauri::command]
pub async fn upload_v2_release(
  app: tauri::AppHandle,
  cancel_map: tauri::State<'_, UploadCancelMap>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  name: String,
  path: String,
) -> Result<(), String> {
  // Guard: refuse to start if an incomplete upload for this name already exists.
  {
    let cfg = app_config.lock().await;
    if let Some(ref p) = cfg.progress_upload {
      if !p.name.is_empty() && p.name == name && !p.is_completed {
        return Err("UPLOAD_IN_PROGRESS: use continue_upload_v2".to_string());
      }
    }
  }

  // Guard before insert — otherwise contains_key always sees our own entry.
  if cancel_map.lock().unwrap().contains_key(&name) {
    return Err("UPLOAD_ALREADY_RUNNING".to_string());
  }

  let (cancel_tx, _) = broadcast::channel::<()>(1);
  {
    cancel_map.lock().unwrap().insert(name.clone(), cancel_tx.clone());
  }
  scopeguard::defer! { cancel_map.lock().unwrap().remove(&name); };

  let base_dir = Path::new(&path);
  let manifest_path = base_dir.join(MANIFEST_NAME);
  let manifest_content = fs::read_to_string(&manifest_path).map_err(|e| {
    let err = anyhow::anyhow!(e);
    log_full_error(&err);
    format!("Failed to read manifest: {}", err)
  })?;
  let manifest_release: ReleaseManifest = serde_json::from_str(&manifest_content).map_err(|e| {
    let err = anyhow::anyhow!(e);
    log_full_error(&err);
    format!("Failed to parse manifest JSON: {}", err)
  })?;
  let _ = app.emit("upload-progress-get-manifest", &manifest_release);

  // Get api_client (drop Service guard immediately).
  let api_client = {
    let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
    let service_guard = state.lock().await;
    service_guard.api_client.clone()
  };

  let provider_manifest = {
    let api = api_client.current_provider().map_err(|e| { log_full_error(&e); e.to_string() })?;
    api.get_manifest()
  }.map_err(|e| { log_full_error(&e); e.to_string() })?;

  upload_log(&app, format!("Start upload_release, max_size: {} path: {}", &provider_manifest.max_size, &path));

  // Resolve release + project_id.
  let releases = {
    let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
    let mut service_guard = state.lock().await;
    service_guard.get_releases(false).await
  }.map_err(|e| { log_full_error(&e); e.to_string() })?;

  let release = releases
    .iter()
    .find(|r| r.name == name)
    .cloned()
    .ok_or_else(|| format!("upload_release(), Release by name '{}' not found!", &name))?;

  upload_log(&app, format!("Found release: {} ({})", &release.name, &release.id));

  let api = api_client.current_provider().map_err(|e| { log_full_error(&e); e.to_string() })?;

  let main_repos = api.get_release_repos_by_name(&release.name).await.map_err(|e| { log_full_error(&e); e.to_string() })?;
  let project = main_repos.first().ok_or_else(|| format!("No repositories found for release '{}'", &release.name))?;
  let project_id = if api.is_suppot_subgroups() { project.id.to_string() } else { project.name.clone() };

  let tag_name = make_tag_name(&name);

  // Compute release_id for set_release_visibility (needed at finalize step).
  let release_id = if api.is_suppot_subgroups() {
    release.path.clone()
  } else {
    release.name.clone()
  };

  // Write initial progress to config.
  {
    let mut cfg = app_config.lock().await;
    cfg.progress_upload = Some(VersionProgressUpload {
      name: name.clone(),
      path: path.clone(),
      tag_name: tag_name.clone(),
      project_id: project_id.clone(),
      release_id: release_id.clone(),
      upload_url: String::new(),
      manifest_uploaded: false,
      tag_created: false,
      release_created: false,
      uploaded_files: Vec::new(),
      total_files: manifest_release.files.len() as u32,
      is_completed: false,
    });
    let _ = cfg.save();
  }

  let ctx = UploadContext {
    app: app.clone(),
    app_config: app_config.inner().clone(),
    cancel_tx,
    name: name.clone(),
    base_dir: base_dir.to_path_buf(),
    manifest_content,
    manifest_release,
    tag_name,
    project_id,
    upload_url: Arc::new(Mutex::new(String::new())),
  };

  // Run all steps in order.
  let api_ref = api_client.current_provider().map_err(|e| { log_full_error(&e); e.to_string() })?;
  step_manifest_upload(&ctx, api_ref).await?;
  step_create_tag(&ctx, api_ref).await?;
  step_create_release(&ctx, api_ref).await?;
  step_upload_assets(&ctx, api_ref).await?;
  step_finalize(&ctx, api_ref, release_id).await?;

  Ok(())
}

// ==================================================================
// Command 2: continue_upload_v2 — resume from saved progress.
// ==================================================================
#[tauri::command]
pub async fn continue_upload_v2(
  app: tauri::AppHandle,
  cancel_map: tauri::State<'_, UploadCancelMap>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  name: String,
) -> Result<(), String> {
  // Guard: refuse if another upload command is already running for this name.
  // Without this, two concurrent resume sessions would both read the same
  // uploaded_files list and push duplicate entries for each uploaded file.
  if cancel_map.lock().unwrap().contains_key(&name) {
    return Err("UPLOAD_ALREADY_RUNNING".to_string());
  }

  // Cancel setup.
  let (cancel_tx, _) = broadcast::channel::<()>(1);
  { cancel_map.lock().unwrap().insert(name.clone(), cancel_tx.clone()); }
  scopeguard::defer! { cancel_map.lock().unwrap().remove(&name); };

  // Read saved progress.
  let progress = {
    let cfg = app_config.lock().await;
    cfg.progress_upload.clone()
  };
  let progress = match progress {
    Some(p) if !p.name.is_empty() && p.name == name && !p.is_completed => p,
    _ => return Err(format!("No resume state for '{}'", &name)),
  };

  upload_log(&app, format!("Resume upload for '{}': manifest={} tag={} release={} files={}/{}", &name, progress.manifest_uploaded, progress.tag_created, progress.release_created, progress.uploaded_files.len(), progress.total_files));

  // Recover manifest from disk.
  let base_dir = Path::new(&progress.path);
  let manifest_content = fs::read_to_string(base_dir.join(MANIFEST_NAME)).map_err(|e| {
    let err = anyhow::anyhow!(e);
    log_full_error(&err);
    format!("Failed to read manifest during resume: {}", err)
  })?;
  let manifest_release: ReleaseManifest = serde_json::from_str(&manifest_content).map_err(|e| {
    let err = anyhow::anyhow!(e);
    log_full_error(&err);
    format!("Failed to parse manifest during resume: {}", err)
  })?;

  // Re-emit the manifest so the frontend rebuilds the per-file progress bars
  // (same behavior as a fresh upload_v2_release). Without this, only the
  // currently-uploading file would get a progress bar during resume.
  let _ = app.emit("upload-progress-get-manifest", &manifest_release);

  // Recover api_client.
  let api_client = {
    let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
    let service_guard = state.lock().await;
    service_guard.api_client.clone()
  };
  let api = api_client.current_provider().map_err(|e| { log_full_error(&e); e.to_string() })?;

  // release_id is saved in progress — no need to re-fetch releases (which may not
  // include the unpublished release being uploaded).
  let release_id = progress.release_id.clone();

  let ctx = UploadContext {
    app: app.clone(),
    app_config: app_config.inner().clone(),
    cancel_tx,
    name: name.clone(),
    base_dir: base_dir.to_path_buf(),
    manifest_content,
    manifest_release,
    tag_name: progress.tag_name.clone(),
    project_id: progress.project_id.clone(),
    upload_url: Arc::new(Mutex::new(progress.upload_url.clone())),
  };

  // Run only unfinished steps.
  if !progress.manifest_uploaded {
    step_manifest_upload(&ctx, api).await?;
  }
  if !progress.tag_created {
    step_create_tag(&ctx, api).await?;
  }
  if !progress.release_created {
    step_create_release(&ctx, api).await?;
  }
  step_upload_assets(&ctx, api).await?;
  step_finalize(&ctx, api, release_id).await?;

  Ok(())
}
