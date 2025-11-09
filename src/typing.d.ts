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
declare interface Version {
  id: string;
  name: string;
  path: string;
  installed_updates: string[];
  is_local: boolean;
}
declare interface VersionProgress {
  id: string;
  name: string;
  path: string;
  files: Dict<FileProgress>;
  is_downloaded: boolean;
  file_count: number;
}
declare interface FileProgress {
  id: string;
  name: string;
  path: string;
  is_downloaded: boolean;
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
  [index: string]: unknown;
}
declare interface AppConfig {
  latest_pid: number;
  first_run: boolean;
  install_path: string;
  client_uuid: string;
  vid_modes: string[];
  vid_mode_latest: string;
  log_level: LogLevel;
  run_params: RunParams;
  pack_source_dir: string;
  pack_target_dir: string;
  unpack_source_dir: string;
  unpack_target_dir: string;
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
