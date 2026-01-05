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
use crate::service::files::ServiceFiles;
use crate::service::get_release::ServiceGetRelease;
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

  let handle = app.handle().clone();
  let handle2 = app.handle().clone();
  let handle3 = app.handle().clone();
  let logger = Arc::new(move |msg: &str| {
    log::info!("{}", &msg);
    let _ = handle.emit("upload-log", msg);
  });

  // Создаём сервис
  let service = Service::new(config_arc.clone(), logger);
  let service_arc = Arc::new(Mutex::new(service));
  let service_files_arc = Arc::new(ServiceFiles::new(move |release_name, file_name, bytes, total_bytes, speed| {
    let _ = handle2.emit("download-speed-status", (release_name, file_name, &bytes, &total_bytes, &speed));
  }));
  let service_updater_arc = Arc::new(ServiceUpdater::new(move |release_name, bytes, speed| {
    let _ = handle3.emit("download-launcher-status", (release_name, &bytes, &speed));
  }));
  let service_clone = service_arc.clone();

  let user_data_placeholder = Arc::new(Mutex::new(Option::<UserData>::None));

  log::info!("Init Service Completed");

  let app_handle = app.handle().clone();
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
  app.manage(service_files_arc);
  app.manage(service_updater_arc);
  app.manage(Arc::new(StdMutex::new(HashMap::new())) as CancelMap);

  log::info!("init App State Completed");

  let app_handle_bg = app.handle().clone();
  let user_data_bg = user_data_placeholder.clone();

  tauri::async_runtime::spawn(async move {
    let result = async {
      // 1. Регистрация провайдеров
      {
        let mut service = service_clone.lock().await;
        service.register_all_providers().await?;
        service.load_manifest().await?;

        let releases = service.get_releases().await?;

        {
          let mut config_guard = config_arc_clone.lock().await;
          config_guard.versions = releases.clone();
          config_guard.save()?;

          let _ = app_handle_bg.emit("config-loaded", config_guard.clone());
        }

        let _ = app_handle_bg.emit("versions-loaded", releases);
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
  // 1. Закрываем все окна (опционально, но вежливо)
  let _ = app_handle.webview_windows().iter().for_each(|(_, window)| {
    let _ = window.close();
  });

  // 2. Получаем путь к текущему бинарнику
  let exe_path = env::current_exe().expect("Failed to get executable path");

  // 3. Запускаем новый экземпляр
  match Command::new(exe_path).spawn() {
    Ok(_) => {
      println!("✅ Запущен новый экземпляр приложения");
    }
    Err(e) => {
      eprintln!("❌ Не удалось запустить новый экземпляр: {}", e);
      // Даже если не удалось — всё равно выходим
    }
  }

  // 4. Завершаем текущий процесс
  process::exit(0);
}
