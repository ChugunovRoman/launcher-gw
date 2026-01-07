use tauri::{Builder, Wry};

use crate::handlers;

pub fn register_handlers(app: Builder<Wry>) -> Builder<Wry> {
  app.invoke_handler(tauri::generate_handler![
    // process
    handlers::process::run_game,
    handlers::process::spawn_external_process,
    handlers::process::get_passed_args,
    handlers::process::is_process_alive,
    handlers::process::open_explorer,
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
    handlers::configs::get_tokens,
    handlers::configs::set_default_install_path,
    handlers::configs::set_default_download_path,
    handlers::configs::set_current_game_version,
    handlers::configs::get_upload_manifest,
    handlers::configs::set_current_api_provider,
    handlers::configs::get_api_providers_stats,
    // user.ltx
    handlers::user_ltx::userltx_set_path,
    // service
    handlers::service::ping_all_providers,
    handlers::service::ping_current_provider,
    handlers::service::get_fastest_provider,
    handlers::service::get_launcher_bg,
    handlers::service::set_token_for_provider,
    handlers::service::get_provider_ids,
    handlers::service::check_available_disk_space,
    handlers::service::remove_download_version,
    handlers::service::move_version,
    // releases
    handlers::start_download_version::start_download_version,
    handlers::start_download_version::cancel_download_version,
    handlers::continue_download_version::continue_download_version,
    handlers::release::get_available_versions,
    handlers::release::create_release_repos,
    handlers::release::get_release_manifest,
    handlers::release::get_local_version,
    handlers::release::get_main_version,
    handlers::release::get_installed_versions,
    handlers::release::delete_installed_version,
    handlers::release::has_root_version,
    handlers::release::add_installed_version_from_config,
    handlers::release::add_installed_version_from_local_path,
    handlers::release::clear_progress_version,
    handlers::release::emit_file_list_stats,
    handlers::upload::upload_release,
    handlers::continue_upload::continue_upload,
    // compress
    handlers::compress::create_archive,
    handlers::compress::extract_archive,
    // permissions
    handlers::permissions::allow_pack_mod,
    // updater
    handlers::updater::update,
    handlers::updater::restart_app,
    // logger
    handlers::logger::log_debug,
    handlers::logger::log_info,
    handlers::logger::log_warn,
    handlers::logger::log_error
  ])
}
