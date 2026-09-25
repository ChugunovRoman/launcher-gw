use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum DownloadStatus {
  Init = 0,
  Pause,
  DownloadFiles,
  Unpacking,
  Verifying,
  Error,
}

#[derive(Clone, Serialize)]
pub struct ProgressPayload {
  pub version_name: String,
  pub file_name: String,
  pub bytes_moved: u64,
  pub total_bytes: u64,
  pub percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
  #[serde(default)]
  pub version_name: String,
  pub status: DownloadStatus,
  #[serde(default)]
  pub progress: f32,
  #[serde(default)]
  pub file: String,
  #[serde(default)]
  pub downloaded_files_cnt: u32,
  #[serde(default)]
  pub total_file_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownlaodFileStat {
  #[serde(default)]
  pub name: String,
  #[serde(default)]
  pub unpacked: bool,
  #[serde(default)]
  pub size: Option<u64>,
}

/// How a manifest entry is processed after downloading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestFileKind {
  /// Unpack the zip into the install dir, then delete the archive.
  #[default]
  Zip,
  /// Copy the file as-is into `<install>/<target>` (or `<name>` when target is
  /// empty) — for future engine `.db*`/`.xdb` archives that need no unpacking.
  Raw,
  /// The patch `manifest.json` itself, stored in `files` as an asset; it is
  /// metadata, not download data.
  Manifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseManifestFile {
  #[serde(default)]
  pub name: String,
  #[serde(default)]
  pub size: u64,
  /// SHA-256 of the finished output file (the `dataN.zip` / `.db` itself, not
  /// the archive contents), lowercase hex. None = old manifest without hashes.
  #[serde(default)]
  pub sha256: Option<String>,
  #[serde(default)]
  pub kind: ManifestFileKind,
  /// Raw files only: path inside the install dir, '/' separators.
  #[serde(default)]
  pub target: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReleaseManifest {
  /// 0 = legacy (no hashes), 2 = sha256 + kind/target manifest (this schema).
  #[serde(default)]
  pub schema: u32,
  #[serde(default)]
  pub total_files_count: u32,
  #[serde(default)]
  pub total_size: u64,
  #[serde(default)]
  pub compressed_size: u64,
  #[serde(default)]
  pub files: Vec<ReleaseManifestFile>,
  #[serde(default)]
  pub exe_path: Option<String>,
  // Partial-update patch metadata. Absent (None/empty) for full release
  // manifests, so old manifests keep deserializing unchanged.
  /// Patch name = release tag in the updates repo.
  #[serde(default)]
  pub patch_name: Option<String>,
  /// Previous patch in the chain (None = first patch after a full release).
  #[serde(default)]
  pub base_patch: Option<String>,
  /// Tag of the full release the patch chain is based on.
  #[serde(default)]
  pub base_release_tag: Option<String>,
  /// Files to delete when applying the patch, relative to the game root.
  #[serde(default)]
  pub deleted_files: Vec<String>,
  /// Faction-editor props this patch changes. Carried here so that
  /// re-publishing the index (which rebuilds every entry from the patch
  /// manifests) cannot lose it — see the plan §2.4.
  #[serde(default)]
  pub updated_fields: Vec<String>,
  /// True when the patch touches spawn/level files and old save games stop
  /// working after installing it. Carried for the same reason as
  /// `updated_fields`: a republish rebuilds the index from the manifests.
  #[serde(default)]
  pub breaks_saves: bool,
}

/// Patch metadata passed to the packer when building a patch upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchMeta {
  pub patch_name: String,
  pub base_patch: Option<String>,
  pub base_release_tag: Option<String>,
  #[serde(default)]
  pub deleted_files: Vec<String>,
  #[serde(default)]
  pub updated_fields: Vec<String>,
  /// Recomputed by `upload_patch` from the actual folder contents — see
  /// `patch_collect::scan_save_breaking`. Old `PatchMeta`s default to false.
  #[serde(default)]
  pub breaks_saves: bool,
}

#[derive(Clone, Serialize)]
pub struct CompressProgressPayload {
  pub status: u8,
  pub current_file: String,
  pub total_size: u64,
  pub processed_size: u64,
  pub percentage: f64,
}

#[derive(Clone, Serialize)]
pub struct UploadProgressPayload {
  pub file_name: String,
  pub file_uploaded_size: u64,
  pub file_total_size: u64,
  pub total_uploaded_size: u64,
  pub total_size: u64,
  pub speed: f64,
}

#[derive(Debug)]
pub struct UnzipTask {
  pub file_name: String,
  pub archive_path: PathBuf,
  pub destination_path: PathBuf,
}

/// Post-processing dispatched after a downloaded file passes verification.
/// `Unzip` — extract the zip into `destination_path` (a directory), then the
/// archive is deleted. `Copy` — move a raw file to `destination_path` (a full
/// file path inside the install dir), no unpacking (engine `.db*` archives).
#[derive(Debug)]
pub enum PostProcessTask {
  Unzip(UnzipTask),
  Copy(UnzipTask),
}

/// Error payload of the `download-version-file-error` event.
#[derive(Debug, Clone, Serialize)]
pub struct FileErrorPayload {
  pub version_name: String,
  pub file: String,
  /// One of the FILE_ERR_* constants from consts.rs.
  pub code: String,
  pub message: String,
}

// ---------------------------------------------------------------------------
// Staged progress of long multi-step commands (patch upload today, the full
// release upload later). The event contract is shared with the frontend
// (`typing.d.ts`): names and enum values must not change.
// ---------------------------------------------------------------------------

/// Stages of `upload_patch`, in the order they run.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PatchUploadStage {
  /// Validation, updates repo lookup, base patch, `.gitkeep` of an empty repo.
  Prepare,
  /// `finalize_fe_fragment`.
  FeFragment,
  /// `scan_save_breaking`.
  SaveBreakScan,
  /// Split archives + checksums.
  Packing,
  /// `create_tag` + `create_release`.
  CreateRelease,
  /// Asset upload loop (with server-side hash verification).
  Upload,
  /// `publish_index` + release cache invalidation.
  PublishIndex,
  /// Tagging the game git repositories.
  TagRepos,
}

/// State of one stage. A stage gets `Running` when it starts and one of the
/// others when it ends; `Warning` may also arrive several times in between.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StageState {
  Running,
  Done,
  Skipped,
  Warning,
  Failed,
}

/// Payload of a stage event (`patch-upload-stage` for patches). Generic over
/// the stage enum so other staged commands can reuse it.
#[derive(Debug, Clone, Serialize)]
pub struct StageEventPayload<S: Serialize> {
  /// Routing key for the frontend (the patch tag for patch uploads).
  pub patch_tag: String,
  pub release_name: String,
  pub stage: S,
  pub state: StageState,
  /// Human-readable detail (file, repo, error text).
  pub message: Option<String>,
  /// Machine code for localized hints (see `consts::ERR_*` / `WARN_*`).
  pub code: Option<String>,
}

/// Any payload of a staged command's event plus its routing key, so the
/// frontend can tell concurrent uploads apart (`patch-upload-log`,
/// `patch-upload-files-count`, `patch-upload-progress`).
#[derive(Debug, Clone, Serialize)]
pub struct TaggedPayload<T: Serialize> {
  pub patch_tag: String,
  pub release_name: String,
  #[serde(flatten)]
  pub payload: T,
}

/// `patch-upload-log` body.
#[derive(Debug, Clone, Serialize)]
pub struct LogLinePayload {
  pub message: String,
}

/// `patch-upload-files-count` body.
#[derive(Debug, Clone, Serialize)]
pub struct FilesCountPayload {
  pub done: u32,
  pub total: u32,
}

/// Result of a finished patch upload (return value of `upload_patch`).
#[derive(Debug, Clone, Serialize)]
pub struct PatchUploadResult {
  /// Per-repo outcome of tagging the game repositories with the patch tag.
  pub repos: Vec<crate::utils::patch_collect::RepoTagReport>,
  /// Non-fatal issues (e.g. failed tag pushes).
  pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadFinishedKind {
  Done,
  Failed,
  Cancelled,
}

/// Payload of `patch-upload-finished`: emitted exactly once per `upload_patch`
/// call, whatever the outcome.
#[derive(Debug, Clone, Serialize)]
pub struct PatchUploadFinishedPayload {
  pub patch_tag: String,
  pub release_name: String,
  pub kind: UploadFinishedKind,
  /// Stage that failed / was cancelled; `None` for `Done`.
  pub stage: Option<PatchUploadStage>,
  pub message: Option<String>,
  pub code: Option<String>,
  /// Only for `Done`.
  pub result: Option<PatchUploadResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatchManifestEntry {
  pub name: String,
  pub size: u64,
}

/// Payload of `patch-upload-manifest`: every asset about to be uploaded.
#[derive(Debug, Clone, Serialize)]
pub struct PatchUploadManifestPayload {
  pub patch_tag: String,
  pub release_name: String,
  pub files: Vec<PatchManifestEntry>,
}

/// Payload of `patch-upload-repo-tagged`.
#[derive(Debug, Clone, Serialize)]
pub struct PatchRepoTaggedPayload {
  pub patch_tag: String,
  pub release_name: String,
  pub report: crate::utils::patch_collect::RepoTagReport,
}

/// Sub-status of one asset during the upload stage.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadFileStatus {
  Uploading,
  /// The whole body is sent, waiting for the server response.
  WaitingServer,
  /// Fetching the server-side sha256.
  Verifying,
  /// Hash mismatch: the asset was deleted and is uploaded again.
  Retrying,
  Done,
}

/// Payload of `patch-upload-file-status`.
#[derive(Debug, Clone, Serialize)]
pub struct PatchUploadFileStatusPayload {
  pub patch_tag: String,
  pub release_name: String,
  pub file_name: String,
  pub status: UploadFileStatus,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CollectStage {
  Scan,
  Diff,
  Copy,
  FeFragment,
  Done,
}

/// Payload of `patch-collect-progress`.
#[derive(Debug, Clone, Serialize)]
pub struct PatchCollectProgress {
  pub stage: CollectStage,
  /// Repository being processed (relative path, the source folder name for
  /// the root repo).
  pub repo: Option<String>,
  pub repos_done: u32,
  pub repos_total: u32,
  /// Files copied so far over all repos.
  pub files_done: u32,
  /// Changed paths seen so far over all repos (grows as repos are diffed).
  pub files_total: u32,
}
