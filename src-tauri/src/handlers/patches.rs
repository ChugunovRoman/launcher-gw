use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Instant;
use std::fs;

use bytes::Bytes;
use futures_util::Stream;
use serde::Serialize;
use tauri::Emitter;
use tokio::{fs::File, sync::broadcast, sync::Mutex};
use tokio_util::io::ReaderStream;

use crate::consts::{DEFAULT_BRANCH, MANIFEST_NAME};
use crate::handlers::compress::{pack_split_archives_reported, PackProgress, PackProgressTag};
use crate::handlers::dto::{FilesCountPayload, LogLinePayload, PatchUploadResult, TaggedPayload};
use crate::handlers::dto::{
  PatchManifestEntry, PatchMeta, PatchRepoTaggedPayload, PatchUploadFileStatusPayload, PatchUploadFinishedPayload,
  PatchUploadManifestPayload, PatchUploadStage, ReleaseManifestFile, StageEventPayload, StageState, UploadFileStatus,
  UploadFinishedKind, UploadProgressPayload,
};
use crate::handlers::patch_install::{project_id_for, resolve_updates_project};
use crate::handlers::upload_v2::{UploadCancelMap, build_asset_url, make_tag_name};
use crate::providers::dto::CreateReleaseAsset;
use crate::service::main::Service;
use crate::utils::errors::log_full_error;
use crate::utils::patch_collect::{self, RepoTagReport};

/// Patch archives are tiny compared to full releases; keep the same chunk
/// limit as full releases for consistency (well below the 2 GiB asset limit).
const PATCH_CHUNK_SIZE_MB: u64 = 2000;

/// Progress event of `collect_patch` (`PatchCollectProgress`).
const EVT_PATCH_COLLECT_PROGRESS: &str = "patch-collect-progress";

/// Event names of one staged command. Patch uploads use [`PATCH_UPLOAD_EVENTS`];
/// the full release upload can get its own set later.
#[derive(Debug, Clone, Copy)]
pub struct StageEvents {
  /// Stage start/end event (`StageEventPayload`).
  pub stage: &'static str,
  /// Plain text log event (also mirrored into `launcher.log`).
  pub log: &'static str,
}

const PATCH_UPLOAD_EVENTS: StageEvents = StageEvents {
  stage: "patch-upload-stage",
  log: "patch-upload-log",
};

/// Failure of a stage: (stage, message, code). `code` is one of the
/// `consts::ERR_*` codes or `None`.
type StageFailure<S> = (S, String, Option<String>);

/// Owned routing data of a staged command, cheap to clone into streams and
/// blocking tasks: every tagged event carries `patch_tag` + `release_name`.
#[derive(Clone)]
struct StageTarget {
  app: tauri::AppHandle,
  patch_tag: String,
  release_name: String,
}

impl StageTarget {
  /// Emits `payload` with this target's routing key flattened into it.
  fn emit_tagged<T: Serialize + Clone>(&self, event: &str, payload: T) {
    let _ = self.app.emit(
      event,
      TaggedPayload {
        patch_tag: self.patch_tag.clone(),
        release_name: self.release_name.clone(),
        payload,
      },
    );
  }

  fn emit_log(&self, event: &str, message: String) {
    self.emit_tagged(event, LogLinePayload { message });
  }

  fn emit_files_count(&self, done: u32, total: u32) {
    self.emit_tagged(EVT_PATCH_UPLOAD_FILES_COUNT, FilesCountPayload { done, total });
  }
}

const EVT_PATCH_UPLOAD_FILES_COUNT: &str = "patch-upload-files-count";

/// Emits the stage events of a multi-step command and keeps the warnings it
/// reported. Generic over the stage enum so it is not tied to patches.
///
/// Rule of the event contract: a stage that got warnings ends as `Warning`
/// (message/code of the last warning), otherwise as `Done`.
struct StageReporter<S: Serialize + Copy + PartialEq + std::fmt::Debug> {
  target: StageTarget,
  events: StageEvents,
  /// Last warning (message, code) of every stage that got one.
  last_warning: StdMutex<Vec<(S, String, Option<String>)>>,
  /// All warnings in order, for the command result.
  warnings: StdMutex<Vec<String>>,
}

impl<S: Serialize + Copy + PartialEq + std::fmt::Debug> StageReporter<S> {
  fn new(target: StageTarget, events: StageEvents) -> Self {
    Self {
      target,
      events,
      last_warning: StdMutex::new(Vec::new()),
      warnings: StdMutex::new(Vec::new()),
    }
  }

  fn emit(&self, stage: S, state: StageState, message: Option<String>, code: Option<String>) {
    let _ = self.target.app.emit(
      self.events.stage,
      StageEventPayload {
        patch_tag: self.target.patch_tag.clone(),
        release_name: self.target.release_name.clone(),
        stage,
        state,
        message,
        code,
      },
    );
  }

  /// Text log line: frontend event + `launcher.log`.
  fn log(&self, message: String) {
    log::info!("{}", &message);
    self.target.emit_log(self.events.log, message);
  }

  fn start(&self, stage: S) {
    log::info!("stage {:?}: running", stage);
    self.emit(stage, StageState::Running, None, None);
  }

  /// End of a stage: `Warning` when it reported warnings, `Done` otherwise.
  fn done(&self, stage: S) {
    let last = crate::utils::locks::lock(&self.last_warning)
      .iter()
      .rev()
      .find(|(s, _, _)| *s == stage)
      .map(|(_, m, c)| (m.clone(), c.clone()));
    match last {
      Some((message, code)) => self.emit(stage, StageState::Warning, Some(message), code),
      None => self.emit(stage, StageState::Done, None, None),
    }
  }

  fn skipped(&self, stage: S, message: String, code: Option<&str>) {
    self.log(message.clone());
    self.emit(stage, StageState::Skipped, Some(message), code.map(str::to_string));
  }

  /// Non-fatal issue: `Warning` event + log + the command result's warnings.
  fn warn(&self, stage: S, message: String, code: Option<&str>) {
    log::warn!("stage {:?}: {}", stage, &message);
    self.target.emit_log(self.events.log, format!("WARNING: {}", &message));
    let code = code.map(str::to_string);
    crate::utils::locks::lock(&self.last_warning).push((stage, message.clone(), code.clone()));
    crate::utils::locks::lock(&self.warnings).push(message.clone());
    self.emit(stage, StageState::Warning, Some(message), code);
  }

  /// Fatal error: `Failed` event + log line + `log::error`. Returns the
  /// failure for `Err(..)`.
  fn fail(&self, stage: S, message: impl Into<String>, code: Option<&str>) -> StageFailure<S> {
    let message = message.into();
    log::error!("stage {:?} failed: {} (code: {:?})", stage, &message, code);
    self.target.emit_log(self.events.log, format!("ERROR: {}", &message));
    let code = code.map(str::to_string);
    self.emit(stage, StageState::Failed, Some(message.clone()), code.clone());
    (stage, message, code)
  }

  fn take_warnings(&self) -> Vec<String> {
    std::mem::take(&mut *crate::utils::locks::lock(&self.warnings))
  }
}

/// Cancel-map key of a patch upload. Both `upload_patch` and
/// `cancel_patch_upload` must derive it the same way from the raw patch name.
fn patch_cancel_key(raw: &str) -> String {
  format!("patch:{}", make_tag_name(raw.trim()))
}

fn emit_file_status(target: &StageTarget, file_name: &str, status: UploadFileStatus) {
  let _ = target.app.emit(
    "patch-upload-file-status",
    PatchUploadFileStatusPayload {
      patch_tag: target.patch_tag.clone(),
      release_name: target.release_name.clone(),
      file_name: file_name.to_string(),
      status,
    },
  );
}

/// `true` once a cancel was sent. `Lagged` also means a send happened.
fn cancel_requested(rx: &mut broadcast::Receiver<()>) -> bool {
  !matches!(rx.try_recv(), Err(broadcast::error::TryRecvError::Empty))
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
///
/// Emits `patch-upload-file-status` = `waiting_server` once the body has been
/// handed over completely (the HTTP client polled past the last chunk).
/// Returns `Err(USER_CANCELLED)` when the stream was stopped by a cancel.
#[allow(clippy::too_many_arguments)]
async fn upload_patch_asset_stream(
  target: &StageTarget,
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
  let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
  let cancelled_in_stream = cancelled.clone();
  let mut cancel_rx_for_stream = cancel_rx;
  // Owned handle: the stream is boxed as `dyn Stream + 'static`.
  let target_for_stream = target.clone();

  let progress_stream = async_stream::stream! {
    let mut uploaded = 0u64;
    for await chunk in file_stream {
      if let Ok(()) = cancel_rx_for_stream.try_recv() {
        log::info!("Patch upload of '{}' cancelled mid-stream", &asset_name_for_stream);
        cancelled_in_stream.store(true, std::sync::atomic::Ordering::Relaxed);
        return;
      }
      if let Ok(ref data) = chunk {
        uploaded += data.len() as u64;
        uploaded_for_emit_in_stream.store(uploaded, std::sync::atomic::Ordering::Relaxed);
        let elapsed = start_time.elapsed().as_secs_f64();
        let speed = if elapsed > 0.0 { uploaded as f64 / elapsed } else { 0.0 };
        target_for_stream.emit_tagged("patch-upload-progress", UploadProgressPayload {
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
    // The client asked for more after the last chunk: the whole body is sent.
    emit_file_status(&target_for_stream, &asset_name_for_stream, UploadFileStatus::WaitingServer);
  };
  let boxed_stream: Box<dyn Stream<Item = std::io::Result<Bytes>> + Send + Unpin> = Box::new(Box::pin(progress_stream));

  log::debug!("upload_patch: asset: {} by url: {}", &asset_name, &asset_url);
  let result = api.upload_release_file(&asset_url, total_size, boxed_stream).await;
  if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
    return Err(crate::consts::ERR_USER_CANCELLED.to_string());
  }
  result.map_err(|e| {
    log_full_error(&e);
    format!("upload_release_file '{}' failed: {}", &asset_name, e)
  })?;

  Ok(uploaded_for_emit.load(std::sync::atomic::Ordering::Relaxed))
}

/// Collects a partial-update patch from the game git repositories:
/// committed changes (latest reachable tag -> HEAD) of every repo found
/// under the selected folder. Heavy git/fs work runs on a blocking thread.
/// Progress goes out as `patch-collect-progress`.
#[tauri::command]
pub async fn collect_patch(
  app: tauri::AppHandle,
  source_dir: String,
  exclude_patterns: Vec<String>,
) -> Result<patch_collect::PatchCollectResult, String> {
  log::info!("collect_patch: source_dir: {}, exclude_patterns: {}", source_dir, exclude_patterns.len());

  let result = tokio::task::spawn_blocking(move || {
    let on_progress = |progress: crate::handlers::dto::PatchCollectProgress| {
      let _ = app.emit(EVT_PATCH_COLLECT_PROGRESS, progress);
    };
    patch_collect::collect_patch(std::path::PathBuf::from(source_dir), exclude_patterns, &on_progress)
  })
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

/// Cancels an in-progress patch upload by patch name (raw, as typed).
#[tauri::command]
pub async fn cancel_patch_upload(cancel_map: tauri::State<'_, UploadCancelMap>, patchName: String) -> Result<(), String> {
  let key = patch_cancel_key(&patchName);
  match crate::utils::locks::lock(&cancel_map).get(&key) {
    Some(tx) => {
      log::info!("cancel_patch_upload: cancelling '{}'", &key);
      let _ = tx.send(());
    }
    None => log::warn!("cancel_patch_upload: no running upload for '{}'", &key),
  }
  Ok(())
}

/// `true` when `resolve_updates_project` failed because the release or its
/// updates repo does not exist (as opposed to a network/API error).
fn is_updates_repo_missing(message: &str) -> bool {
  message.starts_with("No updates repo found") || (message.starts_with("Release '") && message.ends_with("' not found"))
}

/// Uploads a patch into the updates repo of a game release.
///
/// Flow: find the updates repo -> detect `base_patch` (latest existing patch
/// release) -> pack the patch folder into split archives with a patch
/// manifest (data*.zip + manifest.json as release assets) -> create tag +
/// release -> upload assets -> tag the game git repositories with the patch
/// tag (anchors the diff base for the next patch).
///
/// Every stage is reported as `patch-upload-stage`; `patch-upload-finished`
/// is emitted exactly once, whatever the outcome. The returned `Err` keeps
/// the previous shape: the bare code for USER_CANCELLED /
/// PATCH_UPLOAD_ALREADY_RUNNING / UPLOAD_HASH_MISMATCH, the error text otherwise.
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
  let target = StageTarget {
    app: app.clone(),
    patch_tag: make_tag_name(patchName.trim()),
    release_name: name.clone(),
  };
  let reporter = StageReporter::new(target.clone(), PATCH_UPLOAD_EVENTS);

  let outcome = run_upload_patch(
    &reporter,
    cancel_map.inner(),
    service.inner(),
    &name,
    &patchName,
    &patchDir,
    gameSourceDir,
    deletedFiles,
    baseReleaseTag,
    updatedFields,
  )
  .await;

  let mut finished = PatchUploadFinishedPayload {
    patch_tag: target.patch_tag.clone(),
    release_name: target.release_name.clone(),
    kind: UploadFinishedKind::Done,
    stage: None,
    message: None,
    code: None,
    result: None,
  };
  let reply = match outcome {
    Ok(result) => {
      finished.result = Some(result.clone());
      Ok(result)
    }
    Err((stage, message, code)) => {
      let is_cancel = code.as_deref() == Some(crate::consts::ERR_USER_CANCELLED);
      finished.kind = if is_cancel { UploadFinishedKind::Cancelled } else { UploadFinishedKind::Failed };
      finished.stage = Some(stage);
      // The frontend matches these codes on the bare `Err` string.
      let reply = match code.as_deref() {
        Some(
          c @ (crate::consts::ERR_USER_CANCELLED
          | crate::consts::ERR_PATCH_UPLOAD_ALREADY_RUNNING
          | crate::consts::ERR_UPLOAD_HASH_MISMATCH),
        ) => c.to_string(),
        _ => message.clone(),
      };
      finished.message = Some(message);
      finished.code = code;
      Err(reply)
    }
  };

  log::info!(
    "upload_patch finished: release: {} patch: {} kind: {:?} stage: {:?} code: {:?}",
    &finished.release_name,
    &finished.patch_tag,
    finished.kind,
    finished.stage,
    finished.code
  );
  let _ = app.emit("patch-upload-finished", finished);
  reply
}

/// Body of [`upload_patch`], split into stages. Every failure is reported by
/// `reporter.fail` (stage event + log) before it is returned.
#[allow(clippy::too_many_arguments)]
async fn run_upload_patch(
  reporter: &StageReporter<PatchUploadStage>,
  cancel_map: &UploadCancelMap,
  service: &Arc<Mutex<Service>>,
  name: &str,
  patch_name: &str,
  patch_dir: &str,
  game_source_dir: Option<String>,
  deleted_files: Vec<String>,
  base_release_tag: Option<String>,
  updated_fields: Option<Vec<String>>,
) -> Result<PatchUploadResult, StageFailure<PatchUploadStage>> {
  use PatchUploadStage as Stage;
  let app = &reporter.target.app;
  let tag_name = reporter.target.patch_tag.clone();

  // ------------------------------------------------------------------
  // 1. Prepare: validate, find the updates repo, detect base_patch.
  // ------------------------------------------------------------------
  // Cancel map guard (keyed distinctly from full-release uploads). The
  // receiver is subscribed before the key is visible, so a cancel sent at any
  // later point is seen by the check before each file.
  let cancel_key = patch_cancel_key(patch_name);
  let (cancel_tx, mut cancel_rx) = broadcast::channel::<()>(1);
  let already_running = {
    let mut map = crate::utils::locks::lock(cancel_map);
    if map.contains_key(&cancel_key) {
      true
    } else {
      map.insert(cancel_key.clone(), cancel_tx.clone());
      false
    }
  };
  if already_running {
    // Checked BEFORE the first stage event: a duplicate start must not emit
    // `prepare running`, which the frontend treats as the start of a new
    // upload of that release. Only `patch-upload-finished` reports it.
    let message = format!("Upload of patch '{}' is already running", &tag_name);
    log::warn!("upload_patch: {}", &message);
    return Err((Stage::Prepare, message, Some(crate::consts::ERR_PATCH_UPLOAD_ALREADY_RUNNING.to_string())));
  }
  scopeguard::defer! { crate::utils::locks::lock(cancel_map).remove(&cancel_key); };

  reporter.start(Stage::Prepare);

  if patch_name.trim().is_empty() {
    return Err(reporter.fail(Stage::Prepare, "Patch name must not be empty", None));
  }
  if !Path::new(patch_dir).is_dir() {
    return Err(reporter.fail(Stage::Prepare, format!("Patch dir does not exist: {}", patch_dir), None));
  }


  // Get api_client (drop Service guard immediately, mirrors upload_v2).
  let api_client = {
    let service_guard = service.lock().await;
    service_guard.api_client.clone()
  };
  let api = api_client.current_provider().map_err(|e| {
    log_full_error(&e);
    reporter.fail(Stage::Prepare, e.to_string(), None)
  })?;

  reporter.log(format!("Uploading patch '{}' for release '{}' ...", &tag_name, name));

  let updates_project = resolve_updates_project(&api_client, name).await.map_err(|e| {
    log_full_error(&e);
    let message = e.to_string();
    let code = is_updates_repo_missing(&message).then_some(crate::consts::ERR_UPDATES_REPO_NOT_FOUND);
    reporter.fail(Stage::Prepare, message, code)
  })?;
  let project_id = project_id_for(&api_client, &updates_project).map_err(|e| {
    log_full_error(&e);
    reporter.fail(Stage::Prepare, e.to_string(), None)
  })?;

  reporter.log(format!("Updates repo: {}", &project_id));

  // Detect base_patch = latest existing patch release in the chain.
  let mut repo_releases = api.get_repo_releases(&project_id).await.map_err(|e| {
    log_full_error(&e);
    reporter.fail(Stage::Prepare, e.to_string(), None)
  })?;
  // Newest first (None sorts last).
  repo_releases.sort_by(|a, b| b.created_at.cmp(&a.created_at));
  let base_patch = repo_releases.first().map(|r| r.tag_name.clone());
  let already_exists = repo_releases.iter().any(|r| r.tag_name == tag_name);
  if let Some(bp) = &base_patch {
    reporter.log(format!("Base patch: {}", bp));
  } else {
    reporter.log("First patch after full release".to_string());
  }

  // Ensure the updates repo has at least one commit so that
  // create_tag (GitLab, which needs ref=master) and create_release
  // (GitHub, which needs target_commitish) work on freshly created
  // empty repos.
  if base_patch.is_none() {
    reporter.log("Initializing empty updates repo ...".to_string());
    let _ = api
      .add_file_to_repo(&project_id, ".gitkeep", "", "Initialize updates repo", DEFAULT_BRANCH)
      .await;
  }
  reporter.done(Stage::Prepare);

  // ------------------------------------------------------------------
  // 2. Faction editor settings fragment.
  // ------------------------------------------------------------------
  // Name the faction-editor fragment after the patch and drop the props the
  // developer unticked. The collector could not do this: the patch tag only
  // exists here. Failure is not fatal — the patch itself still ships.
  reporter.start(Stage::FeFragment);
  let fe_updated_fields = match finalize_fe_fragment(Path::new(patch_dir), &tag_name, updated_fields.as_deref()) {
    Ok(fields) => {
      if !fields.is_empty() {
        reporter.log(format!("Faction editor settings in this patch: {}", fields.join(", ")));
        reporter.done(Stage::FeFragment);
      } else if fe_fragment_staged(Path::new(patch_dir)) {
        // Only reachable now when the developer unticked every prop, since an
        // absent selection ships everything. Still worth saying out loud.
        reporter.skipped(
          Stage::FeFragment,
          "A faction editor settings fragment is staged in this folder, but every prop was unticked — the patch ships without settings.".to_string(),
          None,
        );
      } else {
        reporter.skipped(Stage::FeFragment, "No faction editor settings in this patch".to_string(), None);
      }
      fields
    }
    Err(e) => {
      reporter.warn(Stage::FeFragment, format!("Faction editor settings were skipped: {}", e), None);
      reporter.done(Stage::FeFragment);
      Vec::new()
    }
  };

  // ------------------------------------------------------------------
  // 3. Save-breaking scan.
  // ------------------------------------------------------------------
  // Source of truth for the save-breaking flag: re-scan the actual folder and
  // the delete list. `upload_patch` receives a folder the collector may have
  // built in an earlier session (or that was touched by hand), so the collect
  // result cannot be trusted here. The walk is cheap — the packer reads the
  // same files right after.
  reporter.start(Stage::SaveBreakScan);
  let save_breaking_files = patch_collect::scan_save_breaking(Path::new(patch_dir), &deleted_files);
  let breaks_saves = !save_breaking_files.is_empty();
  if breaks_saves {
    let shown = save_breaking_files.iter().take(20).cloned().collect::<Vec<_>>().join(", ");
    reporter.warn(
      Stage::SaveBreakScan,
      format!(
        "This patch breaks existing save games, marker files ({}): {}",
        save_breaking_files.len(),
        shown
      ),
      Some(crate::consts::WARN_SAVE_BREAKING),
    );
  }
  reporter.done(Stage::SaveBreakScan);

  // ------------------------------------------------------------------
  // 4. Pack the patch folder into split archives + patch manifest.
  // ------------------------------------------------------------------
  reporter.start(Stage::Packing);
  // Next to the patch, not in %TEMP%: a failed or partial upload then leaves
  // a ready archive the developer can re-upload by hand.
  let pack_dir = Path::new(patch_dir).join(crate::consts::PATCH_ARCHIVE_DIR);
  let pack_dir_str = pack_dir.to_string_lossy().into_owned();

  let patch_meta = PatchMeta {
    patch_name: tag_name.clone(),
    base_patch: base_patch.clone(),
    base_release_tag: base_release_tag.filter(|s| !s.is_empty()),
    deleted_files,
    updated_fields: fe_updated_fields,
    breaks_saves,
  };

  reporter.log("Packing patch archives ...".to_string());
  let (mut manifest, skipped_files) = pack_split_archives_reported(
    app,
    PackProgress {
      event: crate::consts::EVT_PATCH_PACK_PROGRESS,
      tag: Some(PackProgressTag {
        patch_tag: reporter.target.patch_tag.clone(),
        release_name: reporter.target.release_name.clone(),
      }),
      hash_progress: true,
    },
    patch_dir.to_string(),
    pack_dir_str.clone(),
    PATCH_CHUNK_SIZE_MB,
    patch_pack_excludes(),
    None,
    Some(patch_meta),
    Vec::new(),
  )
  .await
  .map_err(|e| reporter.fail(Stage::Packing, e, None))?;

  if !skipped_files.is_empty() {
    reporter.warn(
      Stage::Packing,
      format!(
        "{} unreadable file(s) were left out of the patch: {}",
        skipped_files.len(),
        skipped_files.join(", ")
      ),
      Some(crate::consts::WARN_PACK_SKIPPED_FILES),
    );
  }

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
  reporter.log(format!("Patch archive: {}", pack_dir_str));

  let _ = app.emit(
    "patch-upload-manifest",
    PatchUploadManifestPayload {
      patch_tag: tag_name.clone(),
      release_name: name.to_string(),
      files: manifest
        .files
        .iter()
        .map(|f| PatchManifestEntry {
          name: f.name.clone(),
          size: f.size,
        })
        .collect(),
    },
  );
  reporter.done(Stage::Packing);

  // ------------------------------------------------------------------
  // 5. Create tag + release.
  // ------------------------------------------------------------------
  reporter.start(Stage::CreateRelease);
  if already_exists {
    reporter.log(format!("Tag '{}' already exists (retry after interrupted upload), skipping tag creation", &tag_name));
  } else {
    reporter.log(format!("Creating tag '{}' in updates repo ...", &tag_name));
    if let Err(e) = api.create_tag(&project_id, &tag_name, DEFAULT_BRANCH).await {
      reporter.warn(Stage::CreateRelease, format!("create_tag '{}' failed (may already exist): {}", &tag_name, e), None);
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

  reporter.log(format!("Creating release '{}' ...", &tag_name));
  let created_release = match api.create_release(&project_id, &tag_name, first_assets).await {
    Ok(r) => r,
    Err(e) => {
      if already_exists {
        return Err(reporter.fail(
          Stage::CreateRelease,
          format!(
            "Release '{}' already exists from a previous interrupted upload. \
             Delete the release and tag '{}' manually in the updates repo, then retry. \
             Original error: {}",
            &tag_name, &tag_name, e
          ),
          Some(crate::consts::ERR_RELEASE_EXISTS),
        ));
      }
      return Err(reporter.fail(Stage::CreateRelease, format!("create_release '{}' failed: {}", &tag_name, e), None));
    }
  };
  let upload_template = created_release.upload_url;
  reporter.done(Stage::CreateRelease);

  // ------------------------------------------------------------------
  // 6. Upload every asset (with server-side hash verification).
  // ------------------------------------------------------------------
  reporter.start(Stage::Upload);
  let grand_total: u64 = manifest.files.iter().map(|f| f.size).sum();
  let total_count = manifest.files.len() as u32;
  let mut done_count: u32 = 0;
  let mut uploaded_before: u64 = 0;
  reporter.target.emit_files_count(done_count, total_count);

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
      if cancel_requested(&mut cancel_rx) {
        return Err(reporter.fail(
          Stage::Upload,
          format!("Patch upload cancelled before file: {}", &file.name),
          Some(crate::consts::ERR_USER_CANCELLED),
        ));
      }

      emit_file_status(&reporter.target, &asset_name, UploadFileStatus::Uploading);
      let actually_uploaded = upload_patch_asset_stream(
        &reporter.target,
        api,
        &file_path,
        asset_name.clone(),
        asset_url.clone(),
        total_size,
        uploaded_before,
        grand_total,
        cancel_tx.subscribe(),
      )
      .await
      .map_err(|e| {
        if e == crate::consts::ERR_USER_CANCELLED {
          reporter.fail(
            Stage::Upload,
            format!("Patch upload cancelled during file: {}", &asset_name),
            Some(crate::consts::ERR_USER_CANCELLED),
          )
        } else {
          reporter.fail(Stage::Upload, e, None)
        }
      })?;

      if actually_uploaded < total_size {
        return Err(reporter.fail(
          Stage::Upload,
          format!("Upload of '{}' was interrupted ({} of {} bytes)", &asset_name, actually_uploaded, total_size),
          Some(crate::consts::ERR_USER_CANCELLED),
        ));
      }

      let Some(expected) = file.sha256.as_deref().filter(|s| !s.is_empty()) else {
        break;
      };

      emit_file_status(&reporter.target, &asset_name, UploadFileStatus::Verifying);
      match api.get_uploaded_asset_sha256(&project_id, &tag_name, &asset_name).await {
        Ok(Some(remote)) if remote.eq_ignore_ascii_case(expected) => {
          reporter.log(format!("File {}: sha256 verified on server", &asset_name));
          break;
        }
        Ok(Some(remote)) => {
          verify_attempts += 1;
          if verify_attempts > crate::consts::MAX_UPLOAD_VERIFY_RETRIES {
            return Err(reporter.fail(
              Stage::Upload,
              format!("File {}: server sha256 {} != local {} after {} attempts", &asset_name, &remote, expected, crate::consts::MAX_UPLOAD_VERIFY_RETRIES),
              Some(crate::consts::ERR_UPLOAD_HASH_MISMATCH),
            ));
          }
          reporter.log(format!("File {}: server sha256 {} != local {}, deleting asset and re-uploading (attempt {}/{})", &asset_name, &remote, expected, verify_attempts, crate::consts::MAX_UPLOAD_VERIFY_RETRIES));
          if let Err(e) = api.delete_release_asset(&project_id, &tag_name, &asset_name).await {
            // Fatal: see the identical comment in upload_v2.rs — GitHub
            // rejects a re-upload under an existing asset name, so a failed
            // delete must not be swallowed as a warning.
            return Err(reporter.fail(
              Stage::Upload,
              format!("File {}: failed to delete stale asset before re-upload: {}", &asset_name, e),
              Some(crate::consts::ERR_UPLOAD_HASH_MISMATCH),
            ));
          }
          emit_file_status(&reporter.target, &asset_name, UploadFileStatus::Retrying);
        }
        Ok(None) => {
          reporter.log(format!("File {}: server returned no sha256, verification skipped", &asset_name));
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
    emit_file_status(&reporter.target, &asset_name, UploadFileStatus::Done);
    reporter.target.emit_files_count(done_count, total_count);
    reporter.log(format!("File {} uploaded successful !", &asset_name));
  }

  reporter.log(format!("Patch '{}' uploaded successful !", &tag_name));
  reporter.done(Stage::Upload);

  // ------------------------------------------------------------------
  // 7. Re-publish the static release index. Non-fatal but visible.
  // ------------------------------------------------------------------
  reporter.start(Stage::PublishIndex);
  if let Err(e) = crate::service::index_publisher::publish_index(api, false).await {
    reporter.warn(
      Stage::PublishIndex,
      format!("Failed to publish release index: {}. The patch may not appear for players until the index is re-published manually.", e),
      Some(crate::consts::WARN_INDEX_PUBLISH_FAILED),
    );
  }

  // Invalidate AFTER publishing (see the same note in upload_v2.rs).
  {
    let mut svc = service.lock().await;
    svc.invalidate_releases();
  }
  reporter.done(Stage::PublishIndex);

  // ------------------------------------------------------------------
  // 8. Tag the game git repositories with the patch tag (anchors the
  //    diff base for the NEXT patch). Never aborts the finished upload.
  // ------------------------------------------------------------------
  reporter.start(Stage::TagRepos);
  let mut extra_warnings: Vec<String> = Vec::new();
  let repos: Vec<RepoTagReport> = match game_source_dir.as_deref().filter(|s| !s.is_empty()) {
    Some(source) => {
      reporter.log(format!("Tagging game repositories with '{}' ...", &tag_name));
      let source = source.to_string();
      let tag = tag_name.clone();
      let target = reporter.target.clone();
      let repos = tokio::task::spawn_blocking(move || {
        let on_repo = |report: &RepoTagReport| {
          let _ = target.app.emit(
            "patch-upload-repo-tagged",
            PatchRepoTaggedPayload {
              patch_tag: target.patch_tag.clone(),
              release_name: target.release_name.clone(),
              report: report.clone(),
            },
          );
        };
        patch_collect::tag_game_repos(Path::new(&source), &tag, &on_repo)
      })
      .await
      .map_err(|e| reporter.fail(Stage::TagRepos, e.to_string(), None))?;

      for repo in &repos {
        if !repo.pushed {
          reporter.warn(
            Stage::TagRepos,
            format!("repo '{}': {}", repo.repo_rel_path, repo.message.clone().unwrap_or_else(|| "not pushed".to_string())),
            Some(crate::consts::WARN_TAG_PUSH_FAILED),
          );
        }
      }
      reporter.done(Stage::TagRepos);
      repos
    }
    None => {
      let message = "Game source dir not provided: game repositories were not tagged. Next patch may collect duplicates.".to_string();
      reporter.skipped(Stage::TagRepos, message.clone(), Some(crate::consts::SKIP_NO_GAME_SOURCE_DIR));
      extra_warnings.push(message);
      Vec::new()
    }
  };

  // The pack dir is deliberately NOT removed: it is the archive the developer
  // re-uploads by hand when a release ends up half-published. The next pack of
  // this folder cleans it (`cleanup_previous_pack`).
  reporter.log(format!("Archive kept for manual re-upload: {}", pack_dir_str));

  log::info!("upload_patch done: release: {} patch: {} repos tagged: {}", name, &tag_name, repos.len());
  reporter.target.emit_files_count(total_count, total_count);

  let mut warnings = reporter.take_warnings();
  warnings.extend(extra_warnings);
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
