declare enum LogLevel {
  Debug = "debug",
  Info = "info",
  Warn = "warn",
  Error = "error",
}

declare interface Dict<T> {
  [ket: string]: T;
}

declare interface VersionProgressUpload {
  name: string;
  path: string;
  tag_name: string;
  project_id: string;
  release_id: string;
  upload_url: string;
  manifest_uploaded: boolean;
  tag_created: boolean;
  release_created: boolean;
  uploaded_files: string[];
  total_files: number;
  is_completed: boolean;
}
declare interface DownloadProgress {
  version_name: string;
  status: string;
  file: string;
  progress: number;
  downloaded_files_cnt: number;
  total_file_count: number;
}
declare interface VersionFileDownload {
  downloadProgress: number;
  unpackProgress: number;
  downloadedFileBytes: number;
  totalFileBytes: number;
  downloadSpeed: number;
  speedValue: number;
  sfxValue: string;
  // 0 - в очереди на загрузку; 1 - загружается; 2 - распаковывается; 3 - скачан и распакован;
  // 4 - проверка хэша (verifying); 5 - ошибка (код в errorCode)
  status: number;
  errorCode?: string;
}
declare interface InstalledPatch {
  name: string;
  provider_id: string;
  installed_at?: string | null;
  notes?: string | null;
}
declare interface Version {
  id: number;
  name: string;
  path: string;
  installed_path: string;
  download_path: string;
  engine_path: string | null;
  fsgame_path: string | null;
  userltx_path: string | null;
  exe_path?: string;
  installed_updates: InstalledPatch[];
  is_local: boolean;
  manifest?: ReleaseManifest;
  // only js fields
  inProgress: boolean;
  isStoped: boolean;
  wasCanceled: boolean;
  downloadCurrentFile: string;
  downloadProgress: number;
  downloadedFilesCnt: number;
  totalFileCount: number;
  downloadedFileBytes: number;
  downloadSpeed: number;
  speedValue: number;
  sfxValue: string;
  status: string;
  filesProgress: Map<string, VersionFileDownload>;
}
declare interface VersionProgress {
  id: number;
  name: string;
  path: string;
  installed_path: string;
  download_path: string;
  files: Dict<FileProgress>;
  is_downloaded: boolean;
  downloaded_files_cnt: number;
  total_file_count: number;
  manifest?: ReleaseManifest;
}
declare interface FileProgress {
  id: string;
  download_link: string;
  name: string;
  is_downloaded: boolean;
  is_unpacked: boolean;
  size: number;
  total_size: number;
  sha256?: string | null;
  kind?: ManifestFileKind;
  target?: string | null;
  net_retries?: number;
  verify_retries?: number;
  last_error?: string | null;
}
declare interface IndexPreset {
  id: string;
  options: Dict<string>;
  alife: Dict<string>;
}
declare interface RunParams {
  cmd_params: string;
  check_spawner: boolean;
  check_wait_press_any_key: boolean;
  check_without_cache: boolean;
  check_vsync: boolean;
  check_no_staging: boolean;
  windowed_mode: boolean;
  ui_debug: boolean;
  checks: boolean;
  debug_spawn: boolean;
  vid_mode: string;
  render: string;
  lang: string;
  fov: number;
  hud_fov: number;
  god_mode: boolean;
  unlimited_ammo: boolean;
  show_fps: boolean;
  show_ids: boolean;
  font_legacy: boolean;
  scope_type: string;
  selected_preset_id: string;
  apply_preset_on_launch: boolean;
  // User-editable A-Life overrides, written on top of the preset into alife.ltx.
  alife_objects_per_update: number;
  alife_position_update_interval_ms: number;
  alife_process_time: number;
  alife_switch_distance: number;
  // Backend sets it to true on the first explicit save; false keeps
  // preset-only writes into alife.ltx.
  alife_overrides_initialized: boolean;
  [index: string]: unknown;
}
// Game process tracking (backend GameTracker).
declare interface TrackedGame {
  pid: number;
  start_time: number;
  exe_path: string | null;
  version_name: string;
  subst_drive: string | null;
}
declare interface GameStatus {
  running: boolean;
  pid: number | null;
  version_name: string | null;
}
// run_game rejects with { code, detail }; code is one of the
// app.launchError.* localization keys or "unknown".
declare interface LaunchError {
  code: string;
  detail: string;
}

declare interface UserData {
  uuid: string;
  flags: string[];
}

declare interface AppConfig {
  first_run: boolean;
  install_path: string;
  default_installed_path: string;
  default_download_path: string;
  client_uuid: string;
  vid_modes: string[];
  vid_mode_latest: string;
  log_level: LogLevel;
  lang: string;
  run_params: RunParams;
  pack_source_dir: string;
  pack_target_dir: string;
  unpack_source_dir: string;
  unpack_target_dir: string;
  patch_source_dir: string;
  patch_upload_dir: string;
  patch_exclude_patterns: string[];
  versions: Version[];
  versions_provider_id?: string | null;
  choosed_version_path?: string | null;
  selected_version?: string;
  selected_profile?: string;
  apply_key_profile?: boolean | null;
  selected_provider_id?: string;
  installed_versions: Dict<Version>;
  tokens: Dict<string>;
  hide_max_perf_preset_warning: boolean;
  progress_upload?: VersionProgressUpload;
  progress_download: Dict<VersionProgress>;
  tracked_game?: TrackedGame | null;
  bg_etag?: string | null;
  faction_bundle_author?: string | null;
  faction_settings_version?: string | null;
}


declare interface UploadManifest {
  total_files_count: number;
  total_size: number;
  compressed_size: number;
}
declare type ManifestFileKind = "zip" | "raw" | "manifest";
declare interface ReleaseManifestFile {
  name: string;
  size: number;
  sha256?: string | null;
  kind?: ManifestFileKind;
  target?: string | null;
}
declare interface ReleaseManifest {
  schema?: number;
  total_files_count: number;
  total_size: number;
  compressed_size: number;
  files: ReleaseManifestFile[];
  exe_path?: string;
  // Patch fields (present only in patch manifests from updates repos).
  patch_name?: string;
  base_patch?: string;
  base_release_tag?: string;
  deleted_files: string[];
}


declare interface ProviderStatus {
  available: boolean;
  latency_ms: number | null;
}

// StartupState: aggregate phase of all startup sub-tasks.
declare type PhaseStatus = "pending" | "ok" | "error";
declare interface Phase {
  status: PhaseStatus;
  detail?: string;
}
declare interface StartupState {
  providers: Phase;
  releases: Phase;
  user_data: Phase;
  profiles: Phase;
}

declare interface ProgressPayload {
  version_name: string;
  file_name: string;
  bytes_moved: number;
  total_bytes: number;
  percentage: number;
}

declare interface CompressProgressPayload {
  status: number;
  current_file: string;
  total_size: number;
  processed_size: number;
  percentage: number;
}

declare interface UploadProgressPayload {
  file_name: string;
  file_uploaded_size: number;
  file_total_size: number;
  total_uploaded_size: number;
  total_size: number;
  speed: number;
}

declare interface UploadFileData {
  file_uploaded_size: number;
  file_total_size: number;
  progress: number;
  speedValue: number;
  sfxValue: string;
}

// Partial update patches: git collection result (stage 1)
declare type RepoPatchStatus = "collected" | "no_tags" | "no_changes" | "error";
declare interface RepoPatchReport {
  repo_rel_path: string;
  base_tag: string;
  status: RepoPatchStatus;
  changed: number;
  deleted: number;
  message?: string | null;
}
declare interface PatchCollectResult {
  patch_dir: string;
  deleted_files: string[];
  base_tag: string | null;
  repos: RepoPatchReport[];
  changed: number;
  deleted: number;
}

// Partial update patches: upload result (stage 2)
declare interface RepoTagReport {
  repo_rel_path: string;
  tagged: boolean;
  pushed: boolean;
  message?: string | null;
}
declare interface PatchUploadResult {
  repos: RepoTagReport[];
  warnings: string[];
}


// 

declare interface Option {
  label: string;
  value: any;
}
declare interface KeybindingMap {
  action: string;
  key?: string;
  altkey?: string;
}

declare interface KeybindingMapData {
  key?: string;
  altkey?: string;
}
declare interface ProfileItem {
  name: string;
  keybinds: Dict<String, KeybindingMapData>;
}

// Partial update patches: check & install (stage 3)
declare interface PatchInfo {
  name: string;
  notes: string | null;
  size: number | null;
  is_next: boolean;
}
declare interface PatchCheckResult {
  patches: PatchInfo[];
  missing: string[];
}
declare interface PatchInstallProgress {
  stage: "download" | "unpack" | "delete" | "done" | "error";
  version: string;
  file: string;
  file_progress: number;
  total_progress: number;
}

// Integrity check of an installed version (raw files only).
declare interface VerifyReport {
  checked: number;
  ok: number;
  missing: string[];
  size_mismatch: string[];
  hash_mismatch: string[];
  skipped_no_hash: number;
}
declare interface VerifyInstalledProgress {
  version_name: string;
  file: string;
  done_files: number;
  total_files: number;
  done_bytes: number;
  total_bytes: number;
}
// Per-file download error event payload.
declare interface FileErrorPayload {
  version_name: string;
  file: string;
  // hashMismatch | sizeMismatch | network | unpackFailed | copyFailed | verifyFailed
  code: string;
  message: string;
}
// Server-side asset hashes (Get SHA developer tool).
declare interface AssetSha256 {
  name: string;
  size: number | null;
  sha256: string | null;
}

// --- Faction editor settings bundle (.gwfe) ---
// plans/launcher/faction-editor-settings-bundle-plan.md
//
// `FactionBundleManifest`/`FactionBundleManifestFile`/`FactionBundleSummary`
// mirror the on-disk `manifest.json` format byte-for-byte (snake_case, no
// serde rename) — it is a shared file format, not a Tauri IPC payload, and
// keeping the field names identical to the spec (plan §3.3) matters for a
// future non-Rust reader (e.g. a C++ port). Every other faction* type below
// is an ordinary camelCase Tauri command result.
declare interface FactionBundleManifestFile {
  path: string;
  mode: "replace" | "merge_keys";
  size: number;
  sha256: string;
  target?: string;
}
declare interface FactionBundleSummary {
  factions_total: number;
  factions_created: number;
  custom_armament: string[];
  custom_squad_sizes: string[];
  has_relations: boolean;
  has_population: boolean;
  has_point_types: boolean;
}
declare interface FactionBundleManifest {
  schema: number;
  kind: string;
  name: string;
  description: string;
  author: string;
  created_at: string;
  files: FactionBundleManifestFile[];
  summary: FactionBundleSummary;
}
declare interface FactionBundleInspectResult {
  manifest: FactionBundleManifest;
  warnings: string[];
}
declare interface FactionApplyResult {
  outcome: "applied" | "failed" | "rolledBack";
  warnings: string[];
  backupPath: string | null;
}
declare interface FactionProfileItem {
  id: string;
  manifest: FactionBundleManifest;
}
