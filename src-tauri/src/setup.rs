use std::backtrace::Backtrace;
use std::collections::HashMap;
use std::process::Command;
use std::{env, process};
use std::{
  panic,
  sync::{Arc, Mutex as StdMutex},
};
use tokio::sync::Mutex;

use tauri::Manager;
use tauri::{App, Emitter};

use crate::handlers::start_download_version::CancelMap;
use crate::handlers::upload_v2::UploadCancelMap;
use crate::service::files::ServiceFiles;
use crate::service::get_release::ServiceGetRelease;
use crate::service::keybind_manager::KeybindManager;
use crate::service::unpack::ServiceUnpacker;
use crate::service::updater::ServiceUpdater;
use crate::service::wake_detector::WakeDetector;
use crate::utils::errors::log_full_error;
use crate::{
  configs::{AppConfig::AppConfig, GameConfig::GameConfig, TmpLtx, UserLtx},
  logger::Logger,
  service::{client::ServiceClient, dto::UserData, main::Service},
};

pub fn setup_panic_logger(logger: Arc<std::sync::Mutex<Logger>>) {
  panic::set_hook(Box::new(move |info| {
    // Получаем сообщение паники
    let msg = match info.payload().downcast_ref::<&str>() {
      Some(s) => s.to_string(),
      None => match info.payload().downcast_ref::<String>() {
        Some(s) => s.clone(),
        None => "Box<dyn Any>".to_string(),
      },
    };

    // Место паники (одна строка)
    let location = info
      .location()
      .map(|loc| format!(" at {}:{}:{}", loc.file(), loc.line(), loc.column()))
      .unwrap_or_default();

    // 🔥 Захватываем полный стек вызовов
    let backtrace = Backtrace::force_capture();

    // Формируем полное сообщение
    let full_msg = format!("PANIC: {}{}\n\nStack backtrace:\n{:?}", msg, location, backtrace);

    // Логируем через ваш логгер
    if let Ok(logger_guard) = logger.lock() {
      logger_guard.error(&full_msg);
    }

    // Также выводим в stderr (на случай, если логгер сломан)
    eprintln!("{}", full_msg);
  }));
}

pub fn tauri_setup(app: &mut App) -> Result<(), Box<dyn std::error::Error>> {
  log::info!("Start app setup");

  let config = AppConfig::load_or_create(app.handle())?;
  let config_arc = Arc::new(Mutex::new(config));
  let config_arc_clone = config_arc.clone();

  log::info!("Init AppConfig Completed");

  log::info!("Init user.ltx Completed");

  let user_ltx_config = UserLtx(GameConfig::new(""));
  let tmp_ltx_config = TmpLtx(GameConfig::new(""));

  let handle = Arc::new(app.handle().clone());
  let handle2 = handle.clone();
  let handle3 = handle.clone();
  let handle4 = handle.clone();
  let app_handle = handle.clone();
  let app_handle_bg = handle.clone();

  let logger = Arc::new(move |msg: &str| {
    log::info!("{}", &msg);
    let _ = handle.emit("upload-log", msg);
  });

  let keybind_manager_arc = Arc::new(KeybindManager::new(&handle2));
  let keybind_manager_arc_clone = keybind_manager_arc.clone();

  // Создаём сервис
  let service = Service::new(config_arc.clone(), logger);
  let service_arc = Arc::new(Mutex::new(service));
  let service_unpack_arc = Arc::new(ServiceUnpacker::new(move |release_name, file_name, count, total| {
    let _ = handle2.emit("game-archive-unack-progress", (release_name, file_name, count, total));
  }));
  let service_files_arc = Arc::new(ServiceFiles::new(move |release_name, file_name, bytes, total_bytes, speed| {
    let _ = handle3.emit("download-speed-status", (release_name, file_name, &bytes, &total_bytes, &speed));
  }));
  let service_updater_arc = Arc::new(ServiceUpdater::new(move |release_name, bytes, speed| {
    let _ = handle4.emit("download-launcher-status", (release_name, &bytes, &speed));
  }));
  let service_clone = service_arc.clone();

  let user_data_placeholder = Arc::new(Mutex::new(Option::<UserData>::None));

  log::info!("Init Service Completed");

  let wake_callback = move || {
    restart_app(&app_handle);
  };

  let wake = WakeDetector::new(wake_callback);
  wake.start_watcher(5.0);

  // Регистрируем всё в стейте
  app.manage(config_arc);
  app.manage(Arc::new(Mutex::new(user_ltx_config)));
  app.manage(Arc::new(Mutex::new(tmp_ltx_config)));
  app.manage(user_data_placeholder.clone());
  app.manage(service_arc);
  app.manage(keybind_manager_arc);
  app.manage(service_files_arc);
  app.manage(service_unpack_arc);
  app.manage(service_updater_arc);
  app.manage(Arc::new(StdMutex::new(HashMap::new())) as CancelMap);
  app.manage(Arc::new(StdMutex::new(HashMap::new())) as UploadCancelMap);

  log::info!("init App State Completed");

  let user_data_bg = user_data_placeholder.clone();

  tauri::async_runtime::spawn(async move {
    let result = async {
      // 1. Регистрация провайдеров
      {
        let mut service = service_clone.lock().await;
        service.register_all_providers().await?;
        service.load_manifest().await?;

        let releases = service.get_releases(false).await?;

        {
          let mut config_guard = config_arc_clone.lock().await;
          config_guard.versions = releases.clone();
          config_guard.save()?;

          let _ = app_handle_bg.emit("config-loaded", config_guard.clone());
        }

        let _ = app_handle_bg.emit("versions-loaded", releases);
      }

      {
        keybind_manager_arc_clone.load_profiles().await?;
        let profiles = keybind_manager_arc_clone.get_profiles_str().await;
        let _ = app_handle_bg.emit("load-key-profiles", profiles);
      }

      // 2. Получение данных пользователя
      let data = {
        let guard = config_arc_clone.lock().await;
        (guard.client_uuid.clone(), guard.tokens.clone())
      };
      let user_data = {
        let service_clone_guard = service_clone.lock().await;
        service_clone_guard.set_tokens(data.1).await?;
        service_clone_guard.get_user(data.0).await?
      };
      // Обновляем состояние
      {
        let mut user_data_guard = user_data_bg.lock().await;
        *user_data_guard = Some(user_data);
      }
      log::info!("User data fetched");
      let _ = app_handle_bg.emit("user-data-loaded", ());

      Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Err(e) = result {
      log::error!("Background initialization failed: {:?}", e);
      log_full_error(&e);
      // Опционально: отправить событие в фронтенд
      let _ = app_handle_bg.emit("background-init-failed", e.to_string());
    } else {
      let _ = app_handle_bg.emit("background-init-success", ());
    }
  });

  log::info!("init App Completed");

  Ok(())
}

fn restart_app(app_handle: &tauri::AppHandle) {
  // Flush downloads/uploads before dying so progress is not lost.
  tauri::async_runtime::block_on(crate::handlers::window::graceful_shutdown(app_handle));

  let _ = app_handle.webview_windows().iter().for_each(|(_, window)| {
    let _ = window.close();
  });

  let exe_path = match env::current_exe() {
    Ok(p) => p,
    Err(e) => {
      log::error!("restart_app: cannot get exe path: {}", e);
      process::exit(1);
    }
  };

  match Command::new(exe_path).spawn() {
    Ok(_) => log::info!("restart_app: spawned new instance"),
    Err(e) => log::error!("restart_app: failed to spawn new instance: {}", e),
  }

  process::exit(0);
}
