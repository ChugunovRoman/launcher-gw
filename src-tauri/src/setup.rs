use std::{
  panic,
  path::Path,
  sync::{Arc, Mutex},
};

use tauri::App;
use tauri::Manager;
use tauri::async_runtime;

use crate::{
  configs::{AppConfig::AppConfig, GameConfig::GameConfig, TmpLtx, UserLtx},
  gitlab::{self, client::GitLabClient},
  logger::Logger,
};

pub fn setup_panic_logger(logger: Arc<Mutex<Logger>>) {
  panic::set_hook(Box::new(move |info| {
    let msg = match info.payload().downcast_ref::<&str>() {
      Some(s) => s.to_string(),
      None => match info.payload().downcast_ref::<String>() {
        Some(s) => s.clone(),
        None => "Box<dyn Any>".to_string(),
      },
    };

    let location = info
      .location()
      .map(|loc| format!(" at {}:{}:{}", loc.file(), loc.line(), loc.column()))
      .unwrap_or_default();

    let full_msg = format!("PANIC: {}{}", msg, location);

    if let Ok(logger) = logger.lock() {
      logger.error(&full_msg);
    }

    eprintln!("{}", full_msg);
  }));
}

pub fn tauri_setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
  let config = AppConfig::load_or_create(app.handle())?;
  let working_dir = config.install_path.clone();
  let config_arc = Arc::new(Mutex::new(config.clone()));

  let user_ltx = Path::new(&working_dir).join("appdata").join("user.ltx");
  let tmp_ltx = Path::new(&working_dir).join("appdata").join("tmp.ltx");

  let user_ltx_config = UserLtx(GameConfig::new(&user_ltx));
  let tmp_ltx_config = TmpLtx(GameConfig::new(&tmp_ltx));

  let gl = gitlab::Gitlab::Gitlab::new("https://gitlab.com/api/v4")
    .map_err(|e| log::error!("Cannot init gitlab client, error: {}", e.to_string()))
    .unwrap();
  let gl_arc = Arc::new(Mutex::new(gl.clone()));

  let uuid = &config.client_uuid;
  let user_data = async_runtime::block_on(async { gl.get_user(uuid).await })
    .map_err(|e| {
      log::error!("Failed to fetch user  {}", e);
      e
    })
    .unwrap();

  // Сохраняем в состоянии приложения
  app.manage(config_arc);
  // app.manage(logger_arc);
  app.manage(Arc::new(Mutex::new(user_ltx_config.clone())));
  app.manage(Arc::new(Mutex::new(tmp_ltx_config.clone())));
  app.manage(Arc::new(Mutex::new(user_data.clone())));
  app.manage(gl_arc);

  let versions_dir = Path::new(&working_dir).join("versions");
  std::fs::create_dir_all(versions_dir);

  Ok(())
}
