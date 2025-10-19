use tauri::{Builder, Wry};

use crate::handlers;

pub fn register_handlers(app: Builder<Wry>) -> Builder<Wry> {
  app.invoke_handler(tauri::generate_handler![
    // process
    handlers::process::run_game,
    handlers::process::spawn_external_process,
    handlers::process::get_passed_args,
    handlers::process::is_process_alive,
    // window
    handlers::window::app_exit,
    // configs
    handlers::configs::get_config,
    handlers::configs::save_config,
    handlers::configs::update_run_params,
    // gitlab
    handlers::gitlab::gl_get_bg,
    // logger
    handlers::logger::log_debug,
    handlers::logger::log_info,
    handlers::logger::log_warn,
    handlers::logger::log_error
  ])
}
