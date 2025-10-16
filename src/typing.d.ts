declare enum LogLevel {
  Debug = "debug",
  Info = "info",
  Warn = "warn",
  Error = "error",
}

declare interface RunParams {
  cmd_params: string[];
  check_spawner: boolean;
  check_wait_press_any_key: boolean;
  check_without_cache: boolean;
  check_vsync: boolean;
  check_no_staging: boolean;
  vid_mode: string;
}
declare interface AppConfig {
  first_run: boolean;
  install_path: string;
  client_uuid: string;
  log_level: LogLevel;
  run_params: RunParams;
}
