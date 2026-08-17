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
  // 0 - в очереди на загрузку; 1 - загружается; 2 - распаковывается; 3 - скачаен и распакован
  status: number;
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
  [index: string]: unknown;
}
declare interface AppConfig {
  latest_pid: number;
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
  choosed_version_path?: string | null;
  selected_version?: string;
  selected_profile?: string;
  apply_key_profile?: boolean | null;
  selected_provider_id?: string;
  installed_versions: Dict<Version>;
  tokens: Dict<string>;
  progress_upload?: VersionProgressUpload;
  progress_download: Dict<VersionProgress>;
}


declare interface UploadManifest {
  total_files_count: number;
  total_size: number;
  compressed_size: number;
}
declare interface ReleaseManifestFile {
  name: string;
  size: number;
}
declare interface ReleaseManifest {
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
  stage: "download" | "unpack" | "delete" | "done";
  version: string;
  file: string;
  file_progress: number;
  total_progress: number;
}
