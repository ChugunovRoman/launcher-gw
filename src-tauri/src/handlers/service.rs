use fs_extra::dir::{CopyOptions, TransitProcess, TransitProcessResult, move_dir_with_progress};
use std::{fs, path::{Path, PathBuf}, sync::Arc};
use tauri::Emitter;
use tauri::Manager;
use tokio::sync::Mutex;

use crate::{
  configs::AppConfig::AppConfig,
  handlers::dto::ProgressPayload,
  providers::dto::ProviderStatus,
  service::{main::{ProviderStats, Service}, startup_state::{StartupState, StartupTracker}},
  utils::encoding::*,
};

#[tauri::command]
pub async fn ping_all_providers(
  app: tauri::AppHandle,
  stats_state: tauri::State<'_, ProviderStats>,
) -> Result<Vec<(String, ProviderStatus)>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;

  // Clone api_client under a short lock — network I/O happens outside the mutex.
  let api_client = {
    let service_guard = state.lock().await;
    service_guard.api_client.clone()
  };

  let results: Vec<(String, ProviderStatus)> = api_client
    .ping_all()
    .await
    .into_iter()
    .map(|(id, status)| (id.to_string(), status))
    .collect();

  // Rebuild stats from live provider statuses and store in the shared state.
  {
    let mut stats_guard = stats_state.lock().await;
    let fresh: Vec<_> = api_client
      .get_provider_ids()
      .iter()
      .filter_map(|id| {
        api_client
          .get_provider(id)
          .ok()
          .map(|p| (p.id(), p.status()))
      })
      .collect();
    *stats_guard = fresh;
  }

  Ok(results)
}
#[tauri::command]
pub async fn ping_current_provider(
  app: tauri::AppHandle,
  stats_state: tauri::State<'_, ProviderStats>,
) -> Result<(String, ProviderStatus), String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;

  let (api_client, provider_id) = {
    let service_guard = state.lock().await;
    let api = service_guard.api_client.current_provider().map_err(|e| e.to_string())?;
    (service_guard.api_client.clone(), api.id().to_owned())
  };

  let api = api_client.get_provider(&provider_id).map_err(|e| e.to_string())?;
  let status = api.ping().await;

  // Update shared stats from live provider statuses.
  {
    let mut stats_guard = stats_state.lock().await;
    let fresh: Vec<_> = api_client
      .get_provider_ids()
      .iter()
      .filter_map(|id| {
        api_client
          .get_provider(id)
          .ok()
          .map(|p| (p.id(), p.status()))
      })
      .collect();
    *stats_guard = fresh;
  }

  Ok((provider_id, status))
}

#[tauri::command]
pub async fn get_fastest_provider(app: tauri::AppHandle) -> Result<Option<String>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;
  let fastest = service_guard.api_client.fastest_available();
  Ok(fastest.first().map(|(id, _)| id.to_string()))
}

/// Ping a single provider by id. Updates its live status and shared stats.
#[tauri::command]
pub async fn ping_api_provider(
  app: tauri::AppHandle,
  stats_state: tauri::State<'_, ProviderStats>,
  providerId: String,
) -> Result<(String, ProviderStatus), String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;

  let api_client = {
    let service_guard = state.lock().await;
    service_guard.api_client.clone()
  };

  let api = api_client.get_provider(&providerId).map_err(|e| e.to_string())?;
  let status = api.ping().await;

  // Update shared stats from live provider statuses.
  {
    let mut stats_guard = stats_state.lock().await;
    let fresh: Vec<_> = api_client
      .get_provider_ids()
      .iter()
      .filter_map(|id| {
        api_client
          .get_provider(id)
          .ok()
          .map(|p| (p.id(), p.status()))
      })
      .collect();
    *stats_guard = fresh;
  }

  Ok((providerId, status))
}

#[tauri::command]
pub async fn get_launcher_bg(
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
) -> Result<Vec<u8>, String> {
  let (api_client, provider_id) = {
    let service_guard = service.lock().await;
    let api = service_guard.api_client.current_provider().map_err(|e| e.to_string())?;
    (service_guard.api_client.clone(), api.id().to_string())
  };
  let url = api_client.current_provider().map_err(|e| e.to_string())?.launcher_bg_url();

  // Fast path: index bg_etag matches the saved one -> serve from disk, no network.
  let index_bg_etag = crate::service::index::load_index(&provider_id)
    .await
    .ok()
    .and_then(|i| i.launcher.bg_etag);
  let saved_etag = { app_config.lock().await.bg_etag.clone() };
  if let (Some(idx), Some(saved)) = (&index_bg_etag, &saved_etag)
    && idx == saved
    && let Some(bytes) = crate::utils::http_cache::read_body(&url)
    && !bytes.is_empty()
  {
    log::info!("get_launcher_bg: etag match, serving cached bg (0 network requests)");
    return Ok(bytes);
  }

  // Slow path: fetch via the ETag disk cache.
  let cached = crate::utils::http_cache::fetch(
    &crate::utils::http_cache::SHARED_CLIENT,
    &url,
    std::time::Duration::from_secs(crate::consts::CACHE_TTL_BACKGROUND_SECS),
  )
  .await
  .map_err(|e| format!("Cannot fetch launcher bg: {}", e))?;

  // Persist the served etag for the fast path next time.
  if let Some(etag) = crate::utils::http_cache::read_etag(&url) {
    let mut cfg = app_config.lock().await;
    cfg.bg_etag = Some(etag);
    let _ = cfg.save();
  }

  Ok(cached.bytes)
}

#[tauri::command]
pub async fn set_token_for_provider(app: tauri::AppHandle, token: String, providerId: String) -> Result<(), String> {
  // Validate BEFORE anything is persisted: `set_token` builds an
  // `Authorization` header and fails on a newline or a non-ASCII character (a
  // typical clipboard paste). Persisting such a token reported success to the
  // user while every request went out unauthenticated — on this launch and on
  // every launch after it.
  if !token.is_empty() && reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token)).is_err() {
    log::error!("set_token_for_provider: token for '{}' is not a valid header value", &providerId);

    return Err(crate::consts::ERR_INVALID_TOKEN.to_string());
  }

  {
    // Scoped: the Service guard must not be held across the AppConfig lock or
    // across `clear_all().await`, otherwise every command that needs Service
    // waits behind an unrelated token update.
    let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
    let service_guard = state.lock().await;
    if let Err(e) = service_guard.api_client.get_provider(&providerId) {
      let msg = format!("Cannot get api provider by id {}, error: {:?}", &providerId, e);
      log::error!("{}", msg);

      return Err(msg);
    }
  }

  // Save config FIRST; only then apply the token to the live provider so that
  // a write error does not leave the runtime and persisted state diverged.
  log::info!("set_token_for_provider: id: {}, empty: {}", &providerId, token.is_empty());
  {
    let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("AppConfig not initialized")?;
    let mut cfg_guard = state.lock().await;

    if token.is_empty() {
      cfg_guard.tokens.remove(&providerId);
    } else {
      let encoded_token = encode_token(&token).map_err(|e| e.to_string())?;
      cfg_guard.tokens.insert(providerId.clone(), encoded_token);
    }
    cfg_guard.save().map_err(|e| e.to_string())?;
    log::info!("Save set_token_for_provider");
  }

  // Invalidate disk cache so private responses are not served under the
  // wrong auth context after token change. No Service guard is held here.
  crate::utils::http_cache::clear_all().await;

  // Config persisted successfully — now apply to the live provider.
  {
    let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
    let service_guard = state.lock().await;
    match service_guard.api_client.get_provider(&providerId) {
      Ok(provider) => {
        if let Err(e) = provider.set_token(token) {
          // Non-fatal: the token is saved on disk; next launch will pick it up.
          log::warn!("set_token_for_provider: live set_token failed (will apply on next launch): {}", e);
        }
      }
      Err(e) => log::warn!("set_token_for_provider: provider '{}' is gone: {}", &providerId, e),
    }
  }

  Ok(())
}

#[tauri::command]
pub async fn get_provider_ids(app: tauri::AppHandle) -> Result<Vec<String>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;

  Ok(service_guard.api_client.get_provider_ids())
}

#[tauri::command]
pub async fn check_available_disk_space(path: String, needed: u64) -> Result<bool, String> {
  let path = Path::new(&path);
  let bytes = fs4::available_space(path).map_err(|e| e.to_string())?;

  if bytes > needed {
    return Ok(true);
  }

  Ok(false)
}

#[tauri::command]
pub async fn remove_download_version(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>, versionName: String) -> Result<(), String> {
  let version = {
    let cfg = app_config.lock().await;
    cfg
      .progress_download
      .get(&versionName)
      .cloned()
      .ok_or_else(|| format!("remove_download_version() version not found: {} !", &versionName))?
  };

  // NEVER delete the download dir when it IS the install dir, or contains it:
  // the UI explicitly allows picking the same folder for both, and this
  // cleanup runs right after a SUCCESSFUL install — deleting here would wipe
  // the game that was just installed (and any sibling version, when the
  // download dir is a parent).
  let canon = |p: &str| std::fs::canonicalize(Path::new(p)).unwrap_or_else(|_| PathBuf::from(p));
  let download_dir = canon(&version.download_path);
  let install_dir = canon(&version.installed_path);
  let protects_install =
    !version.installed_path.is_empty() && (download_dir == install_dir || install_dir.starts_with(&download_dir));

  if protects_install {
    log::warn!(
      "remove_download_version: skipping cleanup, download dir '{}' is the install dir (or its parent) '{}'",
      &version.download_path, &version.installed_path
    );
  } else {
    // The download dir may already be absent (already removed in a prior run, or
    // cleared by the user/OS). Cleanup is best-effort: treat NotFound as success
    // instead of bubbling os error 3 up to the frontend, which previously aborted
    // the post-unpack sequence (clear_progress_version never ran, UI hung).
    match fs::remove_dir_all(Path::new(&version.download_path)) {
      Ok(_) => {}
      Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
        log::warn!("remove_download_version: dir already absent: {}", &version.download_path);
      }
      Err(e) => return Err(e.to_string()),
    }
  }

  {
    let mut cfg = app_config.lock().await;
    cfg.progress_download.remove(&versionName);
    cfg.save().map_err(|e| e.to_string())?;
  }

  Ok(())
}

// Cancel-only cleanup of the partial install dir (where archives were being
// unpacked). Reads installed_path from progress_download, so it must be called
// BEFORE remove_download_version/clear_progress_version (which delete that
// entry). Best-effort: tolerate NotFound. NOTE: remove_download_version must
// NOT remove the install dir — it is also called after a successful unpack,
// where the install dir is the installed game.
#[tauri::command]
pub async fn remove_install_dir(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>, versionName: String) -> Result<(), String> {
  let install_path = {
    let cfg = app_config.lock().await;
    cfg
      .progress_download
      .get(&versionName)
      .map(|v| v.installed_path.clone())
      .ok_or_else(|| format!("remove_install_dir: version not found: {}", &versionName))?
  };

  match fs::remove_dir_all(Path::new(&install_path)) {
    Ok(_) => {}
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
      log::warn!("remove_install_dir: dir already absent: {}", &install_path);
    }
    Err(e) => return Err(e.to_string()),
  }

  Ok(())
}

#[tauri::command]
pub async fn move_version(
  app: tauri::AppHandle,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  versionName: String,
  dest: String,
) -> Result<(), String> {
  let version = {
    let cfg = app_config.lock().await;
    cfg
      .installed_versions
      .get(&versionName)
      .ok_or_else(|| format!("move_version() version not found: {} !", &versionName))?
      .clone()
  };

  let mut options = CopyOptions::new();
  options.overwrite = true;
  options.content_only = true;

  // Validate the IPC-provided destination BEFORE the OverwriteAll move:
  // blocks drive roots, system directories and moving a version into itself.
  crate::utils::paths::assert_move_destination(Path::new(&version.installed_path), Path::new(&dest))?;

  // Moving gigabytes is sync IO — run it on the blocking pool so other
  // commands keep responding while the move is in progress.
  let version_name_for_progress = version.name.clone();
  let app_for_progress = app.clone();
  let src = version.installed_path.clone();
  let dest_for_move = dest.clone();
  tokio::task::spawn_blocking(move || {
    move_dir_with_progress(&src, &dest_for_move, &options, move |process_info: TransitProcess| {
      // Guard division by zero for empty dirs (Bug E fix pattern).
      let percentage = if process_info.total_bytes > 0 {
        (process_info.copied_bytes as f64 / process_info.total_bytes as f64) * 100.0
      } else {
        0.0
      };

      let payload = ProgressPayload {
        version_name: version_name_for_progress.clone(),
        file_name: process_info.file_name,
        bytes_moved: process_info.copied_bytes,
        total_bytes: process_info.total_bytes,
        percentage,
      };

      let _ = app_for_progress.emit("move-version", payload);
      TransitProcessResult::OverwriteAll
    })
  })
  .await
  .map_err(|e| e.to_string())?
  .map_err(|e| e.to_string())?;

  let payload = ProgressPayload {
    version_name: version.name.clone(),
    file_name: "".to_owned(),
    bytes_moved: 0,
    total_bytes: 0,
    percentage: 100.,
  };

  let _ = app.emit("move-version", payload);

  {
    let mut cfg = app_config.lock().await;
    let v = cfg
      .installed_versions
      .get_mut(&versionName)
      .ok_or_else(|| format!("move_version() version not found: {} !", &versionName))?;

    let old_path = v.installed_path.clone();

    // Rebase engine_path, fsgame_path, and userltx_path on the new location.
    // Comparison is done component-wise and case-insensitively on Windows:
    // a plain `strip_prefix` on the raw strings left the old location in place
    // whenever the stored paths differed only by drive letter case or by
    // separator style (`C:\x` vs `c:/x`) — and it also matched `..\v1` against
    // `..\v10`.
    let rebase = |old: &Option<String>| -> Option<String> {
      old.as_ref()
        .map(|p| rebase_path(&old_path, &dest, p).unwrap_or_else(|| p.clone()))
    };
    v.engine_path = rebase(&v.engine_path);
    v.fsgame_path = rebase(&v.fsgame_path);
    v.userltx_path = rebase(&v.userltx_path);

    v.installed_path = dest.clone();

    // The in-progress entry keeps its own copy of installed_path and it is the
    // one `remove_install_dir` reads; leaving it at the old location would make
    // the cleanup either fail or delete a directory that no longer belongs to
    // this version.
    if let Some(progress) = cfg.progress_download.get_mut(&versionName) {
      let old_progress_path = progress.installed_path.clone();
      progress.installed_path = rebase_path(&old_path, &dest, &old_progress_path)
        .unwrap_or_else(|| dest.clone());
      log::info!(
        "move_version: progress_download['{}'].installed_path {:?} -> {:?}",
        &versionName, old_progress_path, &progress.installed_path
      );
    }

    cfg.save().map_err(|e| e.to_string())?;
  };

  Ok(())
}

/// Split a path into its non-empty components, ignoring the separator style
/// (`/` and `\` are equivalent) and `.` segments.
fn path_components(path: &str) -> Vec<&str> {
  path.split(['/', '\\']).filter(|c| !c.is_empty() && *c != ".").collect()
}

/// Compare two path components. Windows file names are case-insensitive, so a
/// path stored as `C:\Games` must match `c:\games`; on Unix the comparison
/// stays case-sensitive.
fn components_match(a: &str, b: &str) -> bool {
  if cfg!(windows) { a.eq_ignore_ascii_case(b) } else { a == b }
}

/// Re-root `path` from `old_root` onto `new_root`.
///
/// Returns `None` when `path` is not inside `old_root`, so the caller can keep
/// the original value. The match is component-wise, which - unlike the previous
/// `str::strip_prefix` - is immune to separator style, drive-letter case and to
/// `<...>/v1` being treated as a prefix of `<...>/v10`.
fn rebase_path(old_root: &str, new_root: &str, path: &str) -> Option<String> {
  let root = path_components(old_root);
  let target = path_components(path);

  if root.is_empty() || target.len() < root.len() {
    return None;
  }
  if !root.iter().zip(target.iter()).all(|(a, b)| components_match(a, b)) {
    return None;
  }

  let mut rebased = PathBuf::from(new_root);
  for component in &target[root.len()..] {
    rebased.push(component);
  }

  Some(rebased.to_string_lossy().into_owned())
}

/// Collect release index JSON from live API data for preview.
/// Returns the JSON string to be displayed in a text area.
#[tauri::command]
pub async fn preview_index(app: tauri::AppHandle) -> Result<String, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let api_client = {
    let service_guard = state.lock().await;
    service_guard.api_client.clone()
  };
  let api = api_client.current_provider().map_err(|e| e.to_string())?;

  crate::service::index_publisher::collect_index(api)
    .await
    .map_err(|e| {
      log::error!("preview_index failed: {:?}", e);
      e.to_string()
    })
}

/// Commit a previously previewed index JSON to the provider's index repo.
#[tauri::command]
pub async fn commit_index(app: tauri::AppHandle, json: String) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let api_client = {
    let service_guard = state.lock().await;
    service_guard.api_client.clone()
  };
  let api = api_client.current_provider().map_err(|e| e.to_string())?;

  crate::service::index_publisher::commit_index_json(api, &json)
    .await
    .map_err(|e| {
      log::error!("commit_index failed: {:?}", e);
      e.to_string()
    })?;

  // Invalidate AFTER the commit so a concurrent list request cannot refill
  // the cache from the pre-commit index.
  {
    let mut service_guard = state.lock().await;
    service_guard.invalidate_releases();
  }

  Ok(())
}

/// Re-publish the static release index, skipping the "fewer releases than
/// before" safety check.  Required after a release was deliberately deleted:
/// the normal (non-forced) publish refuses to shrink the index, so without
/// this every automatic publish after an upload would keep failing.
#[tauri::command]
pub async fn republish_index_force(app: tauri::AppHandle) -> Result<(), String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let api_client = {
    let service_guard = state.lock().await;
    service_guard.api_client.clone()
  };
  let api = api_client.current_provider().map_err(|e| e.to_string())?;

  crate::service::index_publisher::publish_index(api, true)
    .await
    .map_err(|e| {
      log::error!("republish_index_force failed: {:?}", e);
      e.to_string()
    })?;

  {
    let mut service_guard = state.lock().await;
    service_guard.invalidate_releases();
  }

  Ok(())
}

/// Names of releases that exist on the provider but are missing from the
/// published static index — i.e. created/uploaded but not yet visible to
/// players (repo still private, or the index was never re-published).
/// Dev-only: without a token the API listing is unavailable and the result is
/// an empty list.
#[tauri::command]
pub async fn get_unpublished_releases(app: tauri::AppHandle) -> Result<Vec<String>, String> {
  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let api_client = {
    let service_guard = state.lock().await;
    service_guard.api_client.clone()
  };
  let api = api_client.current_provider().map_err(|e| e.to_string())?;

  if api.get_token().is_empty() {
    return Ok(vec![]);
  }

  // Same normalization as the API/index merge in refresh_releases: a repo
  // without a description yields "Global-War-Dev" where the index stores
  // "Global War Dev".
  let normalize = |s: &str| s.replace('-', " ").to_lowercase();

  let indexed: std::collections::HashSet<String> = match crate::service::index::load_index(api.id()).await {
    Ok(index) => index.releases.iter().map(|r| normalize(&r.name)).collect(),
    Err(e) => {
      // No index at all — reporting every release as unpublished would be
      // noise, so report nothing and let the log explain why.
      log::warn!("get_unpublished_releases: index unavailable: {}", e);
      return Ok(vec![]);
    }
  };

  let api_releases = api.get_releases(true).await.map_err(|e| e.to_string())?;
  let missing: Vec<String> = api_releases
    .into_iter()
    // The static-index repo lives in the same org but is not a game release;
    // without this it would always be reported as "not published".
    .filter(|r| !r.name.eq_ignore_ascii_case(crate::consts::INDEX_REPO_NAME) && !r.path.eq_ignore_ascii_case(crate::consts::INDEX_REPO_NAME))
    .filter(|r| !indexed.contains(&normalize(&r.name)))
    .map(|r| r.name)
    .collect();

  if !missing.is_empty() {
    log::info!("get_unpublished_releases: {:?}", &missing);
  }

  Ok(missing)
}

/// Return the current startup state (providers, releases, user_data, profiles phases).
#[tauri::command]
pub async fn get_startup_state(tracker: tauri::State<'_, Arc<StartupTracker>>) -> Result<StartupState, String> {
  Ok(tracker.snapshot().await)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn norm(p: &str) -> String {
    PathBuf::from(p).to_string_lossy().into_owned()
  }

  #[test]
  fn rebase_path_handles_mixed_separators_and_drive_case() {
    // The regression: config.json holds `C:\Games\GW\v1\...` while the stored
    // installed_path came back from a folder dialog as `c:/games/gw/v1`.
    let out = rebase_path("c:/games/gw/v1", r"D:\Games\GW\v1", r"C:\Games\GW\v1\bin\xrEngine.exe");
    if cfg!(windows) {
      assert_eq!(out, Some(norm(r"D:\Games\GW\v1\bin\xrEngine.exe")));
    } else {
      // Case-sensitive platforms keep the old (correct) behaviour: no match.
      assert_eq!(out, None);
    }
  }

  #[test]
  fn rebase_path_handles_mixed_separators_with_same_case() {
    let out = rebase_path("C:/Games/GW/v1", r"D:\Games\GW\v1", r"C:\Games\GW\v1\bin\xrEngine.exe");
    assert_eq!(out, Some(norm(r"D:\Games\GW\v1\bin\xrEngine.exe")));
  }

  #[test]
  fn rebase_path_rejects_sibling_with_shared_string_prefix() {
    // `strip_prefix` used to rebase v10 as if it were inside v1.
    assert_eq!(rebase_path(r"C:\Games\v1", r"D:\Games\v1", r"C:\Games\v10\bin"), None);
  }

  #[test]
  fn rebase_path_returns_none_for_unrelated_path() {
    assert_eq!(rebase_path(r"C:\Games\v1", r"D:\Games\v1", r"E:\Other\file.ltx"), None);
  }

  #[test]
  fn rebase_path_maps_the_root_itself() {
    assert_eq!(rebase_path(r"C:\Games\v1", r"D:\Games\v1", "C:/Games/v1"), Some(norm(r"D:\Games\v1")));
  }

  #[test]
  fn rebase_path_ignores_trailing_separators_and_dot_segments() {
    let out = rebase_path(r"C:\Games\v1\", r"D:\Games\v1", r"C:\Games\.\v1\gamedata\configs");
    assert_eq!(out, Some(norm(r"D:\Games\v1\gamedata\configs")));
  }

  #[test]
  fn rebase_path_with_empty_root_keeps_caller_value() {
    assert_eq!(rebase_path("", r"D:\Games\v1", r"C:\Games\v1\bin"), None);
  }
}

