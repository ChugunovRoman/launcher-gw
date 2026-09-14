use std::backtrace::Backtrace;
use std::collections::HashMap;
use std::path::Path;
use std::{
  panic,
  sync::{Arc, Mutex as StdMutex},
};
use tokio::sync::Mutex;

use tauri::Manager;
use tauri::{App, Emitter};

use crate::handlers::patch_install::check_patches_available;
use crate::handlers::start_download_version::CancelMap;
use crate::handlers::upload_v2::UploadCancelMap;
use crate::service::files::ServiceFiles;
use crate::service::game_tracker::{probe, GameTracker};
use crate::service::get_release::{ServiceGetRelease, ReleaseSource};
use crate::service::keybind_manager::KeybindManager;
use crate::service::main::ProviderStats;
use crate::service::startup_state::StartupTracker;
use crate::service::unpack::ServiceUnpacker;
use crate::service::updater::ServiceUpdater;
use crate::service::wake_detector::WakeDetector;
use crate::utils::encoding::{decode_token, encode_token, is_legacy_token};
use crate::utils::errors::log_full_error;
use crate::utils::http_cache;
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

  let (config, mut config_load_error) = match AppConfig::load_or_create(app.handle()) {
    Ok(cfg) => (cfg, None),
    Err(e) => {
      log::error!("AppConfig::load_or_create failed, falling back to in-memory defaults: {}", e);
      let mut defaults = AppConfig::default();
      defaults.first_run = false;
      defaults.path = String::new(); // signals "not persisted"
      (defaults, Some(e.to_string()))
    }
  };
  // NOT `?`: `http_cache::init` resolves the same AppConfig directory as
  // `load_or_create` above, so it fails in exactly the same situations.
  // Returning Err here aborted `tauri_setup` before every `app.manage(...)`
  // below, leaving a window in which all commands answer "Config not
  // initialized" and nothing is reported to the user.
  if let Err(e) = http_cache::init(app.handle()) {
    log::error!("http_cache::init failed, continuing without the HTTP disk cache: {}", e);
    let msg = format!("http_cache::init failed: {}", e);
    config_load_error = Some(match config_load_error {
      Some(prev) => format!("{}; {}", prev, msg),
      None => msg,
    });
  }

  // Capture the saved provider selection and uuid before the config is moved into the Arc.
  let saved_provider_id = config.selected_provider_id.clone();
  let client_uuid = config.client_uuid.clone();

  // Emit a non-fatal event so the frontend can warn the user that the config
  // could not be loaded and is running from in-memory defaults.
  if let Some(err_msg) = config_load_error {
    let _ = app.handle().emit("config-load-error", &err_msg);
  }

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
  let mut service = Service::new(config_arc.clone(), logger);
  // Register providers locally (sync, no network) so get_provider_ids etc.
  // work immediately for the frontend bootstrap.
  service.register_providers_local(saved_provider_id.as_deref());
  // Pre-fill user_data from the cached index so the frontend can call
  // allow_pack_mod immediately, before any network I/O. Use the provider
  // register_providers_local actually resolved (falls back to "github" when
  // nothing was saved yet), not the raw saved_provider_id — otherwise a
  // player who never opened Settings gets no instant value at all.
  let cached_ud = service
    .api_client
    .current_provider()
    .ok()
    .and_then(|api| crate::service::client::cached_user_data_sync(api.id(), &client_uuid));
  // Pre-fill provider stats with placeholders (available=false) so Settings
  // can list both providers right away; real statuses arrive after ping.
  let placeholder_stats: Vec<(&'static str, crate::providers::dto::ProviderStatus)> = service
    .api_client
    .get_provider_ids()
    .iter()
    .filter_map(|id| service.api_client.get_provider(id).ok().map(|p| (p.id(), p.status())))
    .collect();
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

  let user_data_placeholder = Arc::new(Mutex::new(cached_ud));

  log::info!("Init Service Completed");

  let startup_tracker = Arc::new(StartupTracker::new(app.handle().clone()));
  let provider_stats: ProviderStats = Arc::new(Mutex::new(placeholder_stats));

  let wake_callback = move || {
    restart_app(&app_handle);
  };

  let wake = WakeDetector::new(wake_callback);
  wake.start_watcher(5.0);

  // Регистрируем всё в стейте
  let game_tracker_arc = Arc::new(GameTracker::new());
  app.manage(config_arc.clone());
  app.manage(Arc::new(Mutex::new(user_ltx_config)));
  app.manage(Arc::new(Mutex::new(tmp_ltx_config)));
  app.manage(user_data_placeholder.clone());
  app.manage(service_arc);
  app.manage(startup_tracker.clone());
  app.manage(provider_stats.clone());
  app.manage(keybind_manager_arc);
  app.manage(service_files_arc);
  app.manage(service_unpack_arc);
  app.manage(service_updater_arc);
  app.manage(game_tracker_arc.clone());
  app.manage(Arc::new(StdMutex::new(HashMap::new())) as CancelMap);
  app.manage(Arc::new(StdMutex::new(HashMap::new())) as UploadCancelMap);

  log::info!("init App State Completed");

  // Restore the persisted game record (survives launcher restarts while the
  // game is running): probe it once — keep live records, clear dead ones and
  // unmount their leftover subst drives.
  {
    let tracker_restore = game_tracker_arc.clone();
    let config_restore = config_arc.clone();
    let app_restore = app.handle().clone();
    tauri::async_runtime::spawn(async move {
      let game = { config_restore.lock().await.tracked_game.clone() };
      let Some(game) = game else {
        return;
      };

      let probe_game = game.clone();
      let alive = tauri::async_runtime::spawn_blocking(move || probe(&probe_game)).await.unwrap_or(false);

      if alive {
        log::info!("game_tracker: restored live game '{}' (pid {})", game.version_name, game.pid);
        tracker_restore.set(game).await;
        let _ = app_restore.emit("game-status", tracker_restore.status().await);
      } else {
        log::info!(
          "game_tracker: persisted record of '{}' (pid {}) is dead; clearing",
          game.version_name,
          game.pid
        );
        if let Some(drive) = game.subst_drive {
          #[cfg(target_os = "windows")]
          crate::handlers::process::subst_workaround::remove(drive);
        }
        let mut config_guard = config_restore.lock().await;
        config_guard.tracked_game = None;
        if let Err(e) = config_guard.save() {
          log::error!("game_tracker: failed to persist cleared tracked_game: {}", e);
        }
      }
    });
  }

  // Watcher: detects game exit, cleans up subst drives, notifies the frontend.
  crate::service::game_tracker::start_watcher(app.handle().clone(), game_tracker_arc.clone(), config_arc.clone());

  let user_data_bg = user_data_placeholder.clone();
  let startup_tracker_a = startup_tracker.clone();
  let startup_tracker_b = startup_tracker.clone();
  let provider_stats_b = provider_stats.clone();
  let config_arc_clone_b = config_arc_clone.clone();
  let app_handle_bg_b = app_handle_bg.clone();

  // --- Task A: local-only (profiles, no network) ---
  tauri::async_runtime::spawn(async move {
    match keybind_manager_arc_clone.load_profiles().await {
      Ok(()) => {
        {
          let mut cfg = config_arc_clone.lock().await;
          if crate::handlers::profiles::sync_selected_profile(&mut cfg, &keybind_manager_arc_clone).await {
            let _ = cfg.save();
          }
        }
        let profiles = keybind_manager_arc_clone.get_profiles_str().await;
        let _ = app_handle_bg.emit("load-key-profiles", profiles);
        startup_tracker_a.set(|s| s.profiles = crate::service::startup_state::Phase::Ok).await;
      }
      Err(e) => {
        log::error!("load_profiles failed: {:?}", e);
        startup_tracker_a.set(|s| {
          s.profiles = crate::service::startup_state::Phase::Error(e.to_string());
        }).await;
      }
    }
  });

  // --- Task B: network (providers, releases, user data, patches) ---
  tauri::async_runtime::spawn(async move {
    let result = async {
      // Warn about temp dir immediately (no network dependency).
      {
        let install_path = { config_arc_clone_b.lock().await.install_path.clone() };
        let codes = crate::utils::paths::is_temp_path(Path::new(&install_path));
        if !codes.is_empty() {
          log::warn!("launcher runs from a temp directory: {:?} ({:?})", install_path, codes);
          let _ = app_handle_bg_b.emit("launcher-in-temp-dir", codes);
        }
      }

      // 1. Ping providers (no Service lock during network I/O).
      let api_client = {
        let svc = service_clone.lock().await;
        svc.api_client.clone()
      };
      let (stats, best) = crate::service::main::refresh_provider_stats(&api_client).await;
      {
        let mut stats_guard = provider_stats_b.lock().await;
        *stats_guard = stats;
      }
      let _ = app_handle_bg_b.emit("providers-stats", ());

      // Re-select the current provider based on ping results.
      {
        let mut svc = service_clone.lock().await;
        let saved_id = svc.config.lock().await.selected_provider_id.clone();
        let provider_id = match saved_id {
          Some(ref id) => {
            let saved_ok = api_client.get_status(id).map(|s| s.available).unwrap_or(false);
            if saved_ok {
              id.clone()
            } else if let Some(ref fallback) = best {
              log::warn!("Saved provider '{}' unavailable, falling back to '{}'", id, fallback);
              fallback.clone()
            } else {
              id.clone()
            }
          }
          None => best.clone().unwrap_or_else(|| "github".to_string()),
        };
        let _ = svc.api_client.set_current_provider(&provider_id);

        // Persist the actually active provider so the UI and next restart
        // both see the real server, not the stale saved one.
        {
          let mut cfg = svc.config.lock().await;
          if cfg.selected_provider_id.as_ref() != Some(&provider_id) {
            cfg.selected_provider_id = Some(provider_id.clone());
            if let Err(e) = cfg.save() {
              log::warn!("Failed to save fallback provider selection: {}", e);
            }
            let _ = app_handle_bg_b.emit("provider-fallback", &provider_id);
          }
        }
      }

      // Update startup_state: providers done.
      let providers_ok = best.is_some();
      startup_tracker_b.set(|s| {
        s.providers = if providers_ok {
          crate::service::startup_state::Phase::Ok
        } else {
          crate::service::startup_state::Phase::Error("No providers reachable".into())
        };
      }).await;

      // If no providers available — skip network steps but still emit
      // background-init-failed at the end.
      let providers_error = if !providers_ok {
        Some("No available API providers".to_string())
      } else {
        None
      };

      // 1b. Apply the stored provider tokens FIRST.  Steps 2 and 3 below both
      // branch on `get_token().is_empty()`, and get_releases/refresh_releases
      // merge API-only releases only when a token is present.  Applying them
      // later (as part of step 4) meant a dev with a saved PAT always started
      // in anonymous mode: load_manifest skipped, no API merge, and the
      // anonymous GitHub rate limit (60/h) used instead of 5000/h.
      if providers_error.is_none() {
        let tokens = { config_arc_clone_b.lock().await.tokens.clone() };
        if !tokens.is_empty() {
          let svc = service_clone.lock().await;
          if let Err(e) = svc.set_tokens(tokens).await {
            log::warn!("set_tokens (startup) failed: {}", e);
          }
        }
      }

      // 2. load_manifest (conditional, same guard as before).
      if providers_error.is_none() {
        let (is_gitlab, has_token) = {
          let svc = service_clone.lock().await;
          match svc.api_client.current_provider() {
            Ok(api) => (api.is_suppot_subgroups(), !api.get_token().is_empty()),
            Err(_) => (false, false),
          }
        };
        if is_gitlab || has_token {
          let mut svc = service_clone.lock().await;
          if let Err(e) = svc.load_manifest().await {
            log::warn!("load_manifest failed: {}", e);
          }
        } else {
          log::info!("Skipping load_manifest: GitHub player mode (no token)");
        }
      }

      // 3. get_releases.
      if providers_error.is_none() {
        let (releases, provider_id) = {
          let mut svc = service_clone.lock().await;
          let pid = svc.api_client.current_provider().ok().map(|api| api.id().to_string());
          let r = svc.get_releases(ReleaseSource::IndexFirst).await;
          (r, pid)
        };
        match releases {
          Ok(releases) => {
            {
              let mut cfg = config_arc_clone_b.lock().await;
              cfg.versions = releases.clone();
              cfg.versions_provider_id = provider_id;
              let _ = cfg.save();
            }
            let _ = app_handle_bg_b.emit("versions-loaded", releases);
            startup_tracker_b.set(|s| s.releases = crate::service::startup_state::Phase::Ok).await;
          }
          Err(e) => {
            log::warn!("get_releases failed: {}", e);
            startup_tracker_b.set(|s| {
              s.releases = crate::service::startup_state::Phase::Error(e.to_string());
            }).await;
          }
        }
      } else {
        startup_tracker_b.set(|s| {
          s.releases = crate::service::startup_state::Phase::Error(providers_error.clone().unwrap());
        }).await;
      }

      // 4. Parallel: get_user (network) + auto-check patches.
      let user_data_fut = async {
        let data = {
          let guard = config_arc_clone_b.lock().await;
          (guard.client_uuid.clone(), guard.tokens.clone())
        };
        let user_data_result = {
          let svc = service_clone.lock().await;
          if let Err(e) = svc.set_tokens(data.1).await {
            log::warn!("set_tokens failed: {}", e);
          }
          svc.get_user(data.0).await
        };
        let (user_data, user_data_error) = match user_data_result {
          Ok(data) => (data, None),
          Err(e) => (UserData::default(), Some(e.to_string())),
        };
        // Migrate legacy XOR-encoded tokens to the DPAPI-backed storage.
        {
          let mut cfg = config_arc_clone_b.lock().await;
          let mut migrated = false;
          for (id, stored) in cfg.tokens.iter_mut() {
            if is_legacy_token(stored) {
              match decode_token(stored) {
                Ok(plain) => {
                  match encode_token(&plain) {
                    Ok(encoded) => {
                      *stored = encoded;
                      migrated = true;
                      log::info!("Migrated stored token of provider '{}' to DPAPI storage", id);
                    }
                    Err(e) => log::warn!("Token migration to DPAPI skipped for '{}': {}", id, e),
                  }
                }
                Err(e) => log::warn!("Token migration skipped for '{}': {}", id, e),
              }
            }
          }
          if migrated {
            if let Err(e) = cfg.save() {
              log::error!("Failed to persist token migration: {}", e);
            }
          }
        }
        // Update placeholder and emit.
        {
          let mut ud = user_data_bg.lock().await;
          *ud = Some(user_data);
        }
        log::info!("User data fetched");
        let _ = app_handle_bg_b.emit("user-data-loaded", ());
        // Reflect get_user()'s own outcome, not the provider ping result:
        // the index (and therefore user flags) can be reachable via its own
        // stale-fallback cache even when every provider just failed to ping
        // (e.g. github.com unresolvable but raw.githubusercontent.com fine).
        startup_tracker_b.set(|s| {
          s.user_data = match &user_data_error {
            Some(err) => crate::service::startup_state::Phase::Error(err.clone()),
            None => crate::service::startup_state::Phase::Ok,
          };
        }).await;
      };

      let patches_fut = async {
        let api_client = {
          let svc = service_clone.lock().await;
          svc.api_client.clone()
        };
        let version_names: Vec<String> = {
          let cfg = config_arc_clone_b.lock().await;
          cfg.installed_versions.values().map(|v| v.name.clone()).collect()
        };

        // Run patch checks in parallel with 10s timeout each.
        let tasks: Vec<_> = version_names
          .into_iter()
          .map(|vname| {
            let ac = api_client.clone();
            let cfg = config_arc_clone_b.clone();
            async move {
              let check = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                check_patches_available(&ac, &cfg, &vname),
              )
              .await;
              (vname, check)
            }
          })
          .collect();

        for (vname, check) in futures_util::future::join_all(tasks).await {
          match check {
            Ok(Some(count)) if count > 0 => {
              log::info!("Auto-check: {} patches available for '{}'", count, &vname);
              let _ = app_handle_bg_b.emit("patches-available", (&vname, count));
            }
            Ok(Some(_)) => {
              log::info!("Auto-check: '{}' is up to date", &vname);
            }
            Ok(None) => {
              log::warn!("Auto-check: could not check patches for '{}'", &vname);
            }
            Err(_) => {
              log::warn!("Auto-check: timed out for '{}'", &vname);
            }
          }
        }
      };

      tokio::join!(user_data_fut, patches_fut);

      // 5. Emit compatibility events.
      let final_state = startup_tracker_b.snapshot().await;
      let has_error = matches!(
        (&final_state.providers, &final_state.releases),
        (crate::service::startup_state::Phase::Error(_), _) |
        (_, crate::service::startup_state::Phase::Error(_))
      );
      if has_error {
        let _ = app_handle_bg_b.emit("background-init-failed", "Provider or releases error".to_string());
      } else {
        let _ = app_handle_bg_b.emit("background-init-success", ());
      }

      Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Err(e) = result {
      log::error!("Background init (task B) failed: {:?}", e);
      log_full_error(&e);
      let _ = app_handle_bg_b.emit("background-init-failed", e.to_string());
    }
  });

  log::info!("init App Completed");

  Ok(())
}

fn restart_app(app_handle: &tauri::AppHandle) {
  // block_on panics when called from inside the tokio runtime (which is where
  // the wake detector callback runs).  Spawn a dedicated OS thread so the
  // blocking shutdown + restart sequence executes outside the async executor.
  let handle = app_handle.clone();
  std::thread::spawn(move || {
    // Flush downloads/uploads before dying so progress is not lost.
    tauri::async_runtime::block_on(crate::handlers::window::graceful_shutdown(&handle));

    let _ = handle.webview_windows().iter().for_each(|(_, window)| {
      let _ = window.close();
    });

    // Spawns the replacement behind the restart-lock handshake and exits.
    // No self_replace happened on the wake path, so no original_exe override.
    crate::utils::restart::restart_launcher(&handle, None);
  });
}
