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
    handlers::configs::get_lang,
    handlers::configs::set_lang,
    handlers::configs::set_pack_paths,
    handlers::configs::set_unpack_paths,
    // user.ltx
    handlers::user_ltx::userltx_set_path,
    // service
    handlers::service::ping_all_providers,
    handlers::service::get_fastest_provider,
    handlers::service::get_launcher_bg,
    handlers::service::set_token_for_provider,
    // releases
    handlers::release::get_available_versions,
    handlers::release::start_download_version,
    // compress
    handlers::compress::create_archive,
    handlers::compress::extract_archive,
    // permissions
    handlers::permissions::allow_pack_mod,
    // logger
    handlers::logger::log_debug,
    handlers::logger::log_info,
    handlers::logger::log_warn,
    handlers::logger::log_error
  ])
}
