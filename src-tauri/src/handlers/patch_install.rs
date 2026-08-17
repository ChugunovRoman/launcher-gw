use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde::Serialize;
use tauri::Emitter;
use tokio::sync::{Mutex, broadcast};

use crate::configs::AppConfig::AppConfig;
use crate::utils::patch_markers::{InstalledPatch, read_installed_patches, write_patch_marker};
use crate::consts::MANIFEST_NAME;
use crate::handlers::dto::ReleaseManifest;
use crate::handlers::upload_v2::UploadCancelMap;
use crate::providers::ApiClient::ApiClient::ApiClient;
use crate::providers::ApiProvider::ApiProvider;
use crate::providers::dto::Project;
use crate::service::files::{DownloadOutcome, ServiceFiles};
use crate::service::main::Service;
use crate::service::unpack::ServiceUnpacker;
use crate::utils::errors::log_full_error;

/// Patch-related events emitted to the frontend.
const EVT_PATCHES_AVAILABLE: &str = "patches-available";
const EVT_INSTALL_PROGRESS: &str = "patch-install-progress";
const EVT_INSTALL_LOG: &str = "patch-install-log";

#[derive(Debug, Clone, Serialize)]
pub struct PatchInfo {
  pub name: String,
  pub notes: Option<String>,
  pub size: Option<u64>,
  pub is_next: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatchCheckResult {
  pub patches: Vec<PatchInfo>,
  pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PatchInstallProgress {
  /// "download" | "unpack" | "delete" | "done"
  pub stage: String,
  pub version: String,
  pub file: String,
  pub file_progress: f64,
  pub total_progress: f64,
}

fn install_log(app: &tauri::AppHandle, message: String) {
  let _ = app.emit(EVT_INSTALL_LOG, message);
}

// ---------------------------------------------------------------------------
// Shared helper: resolve the updates repo Project for a release.
// ---------------------------------------------------------------------------

/// Resolves the updates repo for a release name using the current API provider.
///
/// For GitLab-like providers the release group id is needed; for GitHub-like
/// providers the release name is passed directly.
pub async fn resolve_updates_project(
  api_client: &ApiClient,
  release_name: &str,
) -> Result<Project> {
  let api = api_client.current_provider()?;
  if api.is_suppot_subgroups() {
    // GitLab: get_updates_repos_by_name expects the release GROUP id.
    let releases = api.get_releases(false).await?;
    let release = releases
      .iter()
      .find(|r| r.name == release_name)
      .ok_or_else(|| anyhow::anyhow!("Release '{}' not found", release_name))?;
    let repos = api.get_updates_repos_by_name(&release.id.to_string()).await?;
    repos
      .into_iter()
      .next()
      .ok_or_else(|| anyhow::anyhow!("No updates repo found for release '{}'", release_name))
  } else {
    let repos = api.get_updates_repos_by_name(release_name).await?;
    repos
      .into_iter()
      .next()
      .ok_or_else(|| anyhow::anyhow!("No updates repo found for release '{}'", release_name))
  }
}

pub fn project_id_for(api_client: &ApiClient, project: &Project) -> Result<String> {
  let api = api_client.current_provider()?;
  Ok(if api.is_suppot_subgroups() {
    project.id.to_string()
  } else {
    project.name.clone()
  })
}

// ---------------------------------------------------------------------------
// Lightweight auto-check: count available patches without downloading manifests.
// Used at startup for the update badge.
// ---------------------------------------------------------------------------

/// Checks how many patches are available for a version (lightweight: no manifest
/// downloads). Returns `Some(count)` or `None` on error.
pub(crate) async fn check_patches_available(
  api_client: &ApiClient,
  app_config: &Arc<Mutex<AppConfig>>,
  version_name: &str,
) -> Option<usize> {
  let installed_path = {
    let cfg = app_config.lock().await;
    cfg
      .installed_versions
      .values()
      .find(|v| v.name == version_name)
      .map(|v| v.installed_path.clone())
  };
  let installed_count = match installed_path {
    Some(ref p) => read_installed_patches(Path::new(p)).len(),
    None => 0,
  };

  let updates_project = match resolve_updates_project(api_client, version_name).await {
    Ok(p) => p,
    Err(e) => {
      log::warn!("Auto-check: cannot resolve updates repo for '{}': {}", version_name, e);
      return None;
    }
  };

  let pid = match project_id_for(api_client, &updates_project) {
    Ok(p) => p,
    Err(_) => return None,
  };

  let api = match api_client.current_provider() {
    Ok(p) => p,
    Err(_) => return None,
  };

  log::info!("Auto-check: querying updates repo '{}' (pid={}) for '{}'", updates_project.name, &pid, version_name);

  let releases = match api.get_repo_releases(&pid).await {
    Ok(r) => {
      log::info!("Auto-check: get_repo_releases returned {} releases for '{}'", r.len(), version_name);
      for rel in &r {
        log::info!("  release: tag='{}' name='{}' assets={}", rel.tag_name, rel.name, rel.assets.len());
      }
      r
    }
    Err(e) => {
      log::warn!("Auto-check: cannot list releases for '{}': {}", version_name, e);
      return None;
    }
  };

  let total = releases.len();
  if total > installed_count {
    Some(total - installed_count)
  } else {
    Some(0)
  }
}

// ---------------------------------------------------------------------------
// get_version_patches
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_version_patches(
  app: tauri::AppHandle,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  versionName: String,
) -> Result<PatchCheckResult, String> {
  let result = get_version_patches_impl(&app, &service, &app_config, &versionName).await;

  match &result {
    Ok(check) => {
      log::info!(
        "get_version_patches '{}': {} patches, {} missing",
        &versionName,
        check.patches.len(),
        check.missing.len()
      );
      let _ = app.emit(EVT_PATCHES_AVAILABLE, (&versionName, check.missing.len()));
    }
    Err(e) => {
      log_full_error(e);
    }
  }

  result.map_err(|e| e.to_string())
}

/// Internal implementation shared by the Tauri command and the auto-check.
pub(crate) async fn get_version_patches_impl(
  app: &tauri::AppHandle,
  service: &tauri::State<'_, Arc<Mutex<Service>>>,
  app_config: &tauri::State<'_, Arc<Mutex<AppConfig>>>,
  version_name: &str,
) -> Result<PatchCheckResult> {
  let api_client = {
    let svc = service.lock().await;
    svc.api_client.clone()
  };

  // Find the installed version and read patch markers from disk.
  let installed_set: HashSet<String> = {
    let cfg = app_config.lock().await;
    let v = cfg
      .installed_versions
      .values()
      .find(|v| v.name == version_name)
      .ok_or_else(|| anyhow::anyhow!("Version '{}' not found in installed_versions", version_name))?;
    read_installed_patches(Path::new(&v.installed_path))
      .into_iter()
      .map(|p| p.name)
      .collect()
  };

  // Find the updates repo.
  let updates_project = resolve_updates_project(&api_client, version_name).await?;
  let pid = project_id_for(&api_client, &updates_project)?;

  log::info!("get_version_patches: querying updates repo '{}' (pid={}) for '{}'", updates_project.name, &pid, version_name);

  // List all releases in the updates repo (now includes assets).
  let releases = api_client.current_provider()?.get_repo_releases(&pid).await?;

  log::info!("get_version_patches: get_repo_releases returned {} releases for '{}'", releases.len(), version_name);
  for rel in &releases {
    log::info!("  release: tag='{}' name='{}' assets={}", rel.tag_name, rel.name, rel.assets.len());
  }

  if releases.is_empty() {
    return Ok(PatchCheckResult {
      patches: vec![],
      missing: vec![],
    });
  }

  // Sort by created_at ascending (chain order).
  let mut sorted_releases = releases;
  sorted_releases.sort_by(|a, b| {
    let ta = a.created_at.as_deref().unwrap_or("");
    let tb = b.created_at.as_deref().unwrap_or("");
    ta.cmp(tb)
  });

  // Build patch info from releases directly (no manifest download needed for listing).
  // tag_name is always the canonical patch name (set during upload).
  let mut found_next = false;
  let mut patches: Vec<PatchInfo> = Vec::new();

  for release in &sorted_releases {
    let name = release.tag_name.clone();
    let is_installed = installed_set.contains(&name);
    let is_next = !is_installed && !found_next;
    if is_next {
      found_next = true;
    }

    let size: Option<u64> = {
      let total: u64 = release
        .assets
        .iter()
        .filter(|a| a.name != MANIFEST_NAME)
        .filter_map(|a| a.size)
        .sum();
      if total > 0 { Some(total) } else { None }
    };

    patches.push(PatchInfo {
      name,
      notes: release.body.clone(),
      size,
      is_next,
    });
  }

  let missing: Vec<String> = patches.iter().filter(|p| !installed_set.contains(&p.name)).map(|p| p.name.clone()).collect();

  Ok(PatchCheckResult { patches, missing })
}

/// Downloads and parses a manifest.json from a release asset URL.
async fn download_manifest(api_client: &ApiClient, url: &str) -> Result<ReleaseManifest> {
  let api = api_client.current_provider()?;
  let stream = api.get_blob_by_url_stream(url, &None).await?;
  let bytes = stream
    .fold(Vec::new(), |mut acc, chunk| async {
      if let Ok(data) = chunk {
        acc.extend_from_slice(&data);
      }
      acc
    })
    .await;
  let manifest: ReleaseManifest = serde_json::from_slice(&bytes)?;
  Ok(manifest)
}

// ---------------------------------------------------------------------------
// start_install_patch
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn start_install_patch(
  app: tauri::AppHandle,
  cancel_map: tauri::State<'_, UploadCancelMap>,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  service_files: tauri::State<'_, Arc<ServiceFiles>>,
  service_unpack: tauri::State<'_, Arc<ServiceUnpacker>>,
  versionName: String,
  patchName: String,
) -> Result<(), String> {
  // Guard: only one install per version at a time.
  let cancel_key = format!("patch-install:{}", &versionName);
  if cancel_map.lock().unwrap().contains_key(&cancel_key) {
    return Err("PATCH_INSTALL_ALREADY_RUNNING".to_string());
  }
  let (cancel_tx, _) = broadcast::channel::<()>(1);
  cancel_map.lock().unwrap().insert(cancel_key.clone(), cancel_tx.clone());
  scopeguard::defer! { cancel_map.lock().unwrap().remove(&cancel_key); };

  let result = start_install_patch_inner(
    &app,
    &cancel_tx,
    &service,
    &app_config,
    &service_files,
    &service_unpack,
    &versionName,
    &patchName,
  )
  .await;

  if let Err(ref e) = result {
    log_full_error(e);
  }

  result.map_err(|e| e.to_string())
}

async fn start_install_patch_inner(
  app: &tauri::AppHandle,
  cancel_tx: &broadcast::Sender<()>,
  service: &tauri::State<'_, Arc<Mutex<Service>>>,
  app_config: &tauri::State<'_, Arc<Mutex<AppConfig>>>,
  service_files: &tauri::State<'_, Arc<ServiceFiles>>,
  service_unpack: &tauri::State<'_, Arc<ServiceUnpacker>>,
  version_name: &str,
  patch_name: &str,
) -> Result<()> {
  let api_client = {
    let svc = service.lock().await;
    svc.api_client.clone()
  };
  let api = api_client.current_provider()?;
  let provider_id = api.id().to_string();

  // Find version and read installed patches from marker files.
  let installed_path = {
    let cfg = app_config.lock().await;
    cfg
      .installed_versions
      .values()
      .find(|v| v.name == version_name)
      .ok_or_else(|| anyhow::anyhow!("Version '{}' not found", version_name))?
      .installed_path
      .clone()
  };
  let last_installed_patch = read_installed_patches(Path::new(&installed_path))
    .last()
    .map(|p| p.name.clone());

  // Resolve updates repo and find the target release.
  let updates_project = resolve_updates_project(&api_client, version_name).await?;
  let pid = project_id_for(&api_client, &updates_project)?;

  let releases = api.get_repo_releases(&pid).await?;
  let release = releases
    .iter()
    .find(|r| r.tag_name == patch_name)
    .ok_or_else(|| anyhow::anyhow!("Patch '{}' not found in updates repo", patch_name))?
    .clone();

  // Download and validate manifest.
  let manifest_asset = release
    .assets
    .iter()
    .find(|a| a.name == MANIFEST_NAME)
    .ok_or_else(|| anyhow::anyhow!("Patch '{}' has no manifest.json", patch_name))?
    .clone();

  let manifest = download_manifest(&api_client, &manifest_asset.download_link).await?;

  // Validate chain: base_patch must match the last installed patch.
  let expected_base = manifest.base_patch.as_deref().unwrap_or("");
  let actual_base = last_installed_patch.as_deref().unwrap_or("");
  if expected_base != actual_base {
    bail!(
      "Patch chain mismatch: patch '{}' expects base '{}', but last installed is '{}'",
      patch_name,
      expected_base,
      actual_base
    );
  }

  install_log(app, format!("Installing patch '{}' for version '{}' ...", patch_name, version_name));

  // Prepare download directory.
  let patches_dir = Path::new(&installed_path).join(".patches").join(patch_name);
  std::fs::create_dir_all(&patches_dir).context("create patch download dir")?;

  let version_name_owned = version_name.to_string();

  // Download data*.zip assets (skip manifest.json).
  let data_assets: Vec<_> = release
    .assets
    .iter()
    .filter(|a| a.name != MANIFEST_NAME && a.name.starts_with("data"))
    .collect();

  let grand_total: u64 = data_assets.iter().filter_map(|a| a.size).sum();
  let mut downloaded_total: u64 = 0;

  for (i, asset) in data_assets.iter().enumerate() {
    // Cancel check.
    if cancel_tx.receiver_count() > 0 {
      let mut probe = cancel_tx.subscribe();
      if probe.try_recv().is_ok() {
        install_log(app, "Patch install cancelled.".to_string());
        return Err(anyhow::anyhow!("USER_CANCELLED"));
      }
    }

    let file_path = patches_dir.join(&asset.name);
    let total_size = asset.size.unwrap_or(0);

    // For GitLab: fetch size via HEAD if not available.
    let actual_size = if total_size == 0 {
      api.get_file_content_size(&asset.download_link).await.unwrap_or(0)
    } else {
      total_size
    };

    install_log(
      app,
      format!("Downloading {} ({}/{})", &asset.name, i + 1, data_assets.len()),
    );

    let file_name = asset.name.clone();

    let outcome = service_files
      .download_blob_to_file(
        &api_client,
        version_name,
        &asset.download_link,
        &actual_size,
        &file_path,
        &None,
        cancel_tx.subscribe(),
      )
      .await?;

    match outcome {
      DownloadOutcome::Completed => {
        downloaded_total += actual_size;
        let _ = app.emit(
          EVT_INSTALL_PROGRESS,
          PatchInstallProgress {
            stage: "download".to_string(),
            version: version_name_owned.clone(),
            file: file_name,
            file_progress: 100.0,
            total_progress: if grand_total > 0 {
              (downloaded_total as f64 / grand_total as f64) * 50.0
            } else {
              50.0
            },
          },
        );
      }
      DownloadOutcome::Interrupted => {
        install_log(app, "Download interrupted.".to_string());
        return Err(anyhow::anyhow!("USER_CANCELLED"));
      }
    }
  }

  // Unpack all archives into the game root.
  install_log(app, "Unpacking archives ...".to_string());
  let mut unpack_progress = 0u32;
  let total_archives = data_assets.len() as u32;
  let svc = service_unpack.inner().clone();

  for asset in &data_assets {
    let archive_path = patches_dir.join(&asset.name);
    let extract_to = PathBuf::from(&installed_path);
    let unpack_name = asset.name.clone();
    let svc = svc.clone();
    let vn = version_name_owned.clone();

    tokio::task::spawn_blocking(move || {
      svc.extract_zip(&vn, &unpack_name, &archive_path, &extract_to)
    })
    .await
    .map_err(|e| anyhow::anyhow!("Unpack task failed: {}", e))?
    .map_err(|e| anyhow::anyhow!("Extract failed: {}", e))?;

    unpack_progress += 1;
    let _ = app.emit(
      EVT_INSTALL_PROGRESS,
      PatchInstallProgress {
        stage: "unpack".to_string(),
        version: version_name_owned.clone(),
        file: asset.name.clone(),
        file_progress: 100.0,
        total_progress: 50.0 + (unpack_progress as f64 / total_archives as f64) * 40.0,
      },
    );
  }

  // Delete files listed in deleted_files (after successful unpack — atomicity).
  install_log(app, "Deleting removed files ...".to_string());
  for (idx, rel_path) in manifest.deleted_files.iter().enumerate() {
    // Guard against path traversal.
    let normalized = rel_path.replace('\\', "/");
    if normalized.contains("..") {
      log::warn!("Skipping deleted_file with '..': {}", rel_path);
      continue;
    }

    let abs_path = Path::new(&installed_path).join(rel_path);
    if abs_path.exists() {
      if let Err(e) = std::fs::remove_file(&abs_path) {
        install_log(app, format!("Warning: cannot delete '{}': {}", rel_path, e));
      }
    } else {
      install_log(app, format!("Note: '{}' already absent", rel_path));
    }

    let _ = app.emit(
      EVT_INSTALL_PROGRESS,
      PatchInstallProgress {
        stage: "delete".to_string(),
        version: version_name_owned.clone(),
        file: rel_path.clone(),
        file_progress: 100.0,
        total_progress: 90.0 + (idx as f64 / manifest.deleted_files.len().max(1) as f64) * 10.0,
      },
    );
  }

  // Record the patch as installed via marker file.
  write_patch_marker(Path::new(&installed_path), &InstalledPatch {
    name: patch_name.to_string(),
    provider_id,
    installed_at: Some(chrono::Local::now().to_rfc3339()),
    notes: release.body.clone(),
  })?;

  // Cleanup: remove the patches dir on success.
  if let Err(e) = std::fs::remove_dir_all(&patches_dir) {
    log::warn!("Cannot remove patch dir {:?}: {}", patches_dir, e);
  }

  install_log(app, format!("Patch '{}' installed successfully!", patch_name));
  let _ = app.emit(
    EVT_INSTALL_PROGRESS,
    PatchInstallProgress {
      stage: "done".to_string(),
      version: version_name_owned.clone(),
      file: String::new(),
      file_progress: 100.0,
      total_progress: 100.0,
    },
  );

  Ok(())
}

// ---------------------------------------------------------------------------
// cancel_install_patch
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn cancel_install_patch(
  cancel_map: tauri::State<'_, UploadCancelMap>,
  versionName: String,
) -> Result<(), String> {
  let key = format!("patch-install:{}", &versionName);
  if let Some(tx) = cancel_map.lock().unwrap().get(&key) {
    let _ = tx.send(());
  }
  Ok(())
}
