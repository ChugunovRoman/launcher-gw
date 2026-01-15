declare enum LogLevel {
  Debug = "debug",
  Info = "info",
  Warn = "warn",
  Error = "error",
}

declare enum UploadState {
  InProgress = "InProgress",
  Completed = "Completed",
}

declare interface Dict<T> {
  [ket: string]: T;
}

declare interface VersionProgressUpload {
  name: string;
  path_dir: string;
  path_repo: string;
  files_per_commit: number;
  total_groups: number;
  uploaded_groups: number;
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
declare interface Version {
  id: string;
  name: string;
  path: string;
  installed_path: string;
  download_path: string;
  installed_updates: string[];
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
  id: string;
  name: string;
  path: string;
  installed_path: string;
  download_path: string;
  files: Dict<FileProgress>;
  is_downloaded: boolean;
  is_unpacked: boolean;
  downloaded_files_cnt: number;
  total_file_count: number;
  manifest?: ReleaseManifest;
}
declare interface FileProgress {
  id: string;
  name: string;
  path: string;
  is_downloaded: boolean;
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
  run_params: RunParams;
  pack_source_dir: string;
  pack_target_dir: string;
  unpack_source_dir: string;
  unpack_target_dir: string;
  selected_version?: string;
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
}

declare interface CommitSyncState {
  files: Dict<string>;
  was_pushed: boolean;
}
declare interface RepoSyncState {
  commits: Dics<CommitSyncState>;
  state: UploadState;
  total_files_count: number;
  uploaded_files_count: number;
}

declare interface ProviderStatus {
  available: boolean;
  latency_ms: number;
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
