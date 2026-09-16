// "Verify integrity" of an installed version (stage 4 of the download
// integrity plan) + "Repair" (re-download of broken files through the normal
// download pipeline).
//
// Only `kind = Raw` files can be verified: for them the manifest hash is the
// hash of the installed file itself. Files unpacked from zips carry the
// ARCHIVE hash, not the content hash — the UI explains this limitation.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::Ordering};

use serde::Serialize;
use tauri::Emitter;
use tokio::sync::Mutex;

use crate::configs::AppConfig::{AppConfig, FileProgress, VersionProgress};
use crate::consts::{ERR_DOWNLOAD_ALREADY_RUNNING, ERR_USER_CANCELLED, ERR_VERIFY_ALREADY_RUNNING};
use crate::handlers::dto::{ManifestFileKind, ReleaseManifest, ReleaseManifestFile};
use crate::handlers::start_download_version::CancelMap;
use crate::service::files::ServiceFiles;
use crate::service::get_release::ServiceGetRelease;
use crate::service::main::Service;
use crate::service::unpack::ServiceUnpacker;

#[derive(Debug, Clone, Serialize)]
pub struct VerifyReport {
  pub checked: u32,
  pub ok: u32,
  pub missing: Vec<String>,
  pub size_mismatch: Vec<String>,
  pub hash_mismatch: Vec<String>,
  pub skipped_no_hash: u32,
}

#[derive(Debug, Clone, Serialize)]
struct VerifyInstalledProgress {
  version_name: String,
  file: String,
  done_files: u32,
  total_files: u32,
  done_bytes: u64,
  total_bytes: u64,
}

/// One reference entry together with the manifest it came from.
///
/// The source matters for "Repair": a file whose hash comes from a patch
/// manifest must be re-downloaded from THAT patch's assets. Taking the release
/// copy instead means the file is verified against the patch hash — a
/// guaranteed HASH_MISMATCH on every one of the three attempts, and the file
/// can never be repaired.
#[derive(Debug, Clone)]
struct FileReference {
  file: ReleaseManifestFile,
  /// Patch tag whose manifest supplied this entry; `None` = release manifest.
  patch: Option<String>,
}

/// Hash reference for the installed files: release manifest overlaid with the
/// manifests of the installed patches in install order (a file replaced by a
/// later patch is verified against THAT patch's entry).
fn build_file_reference(release_manifest: Option<&ReleaseManifest>, installed_path: &Path) -> HashMap<String, FileReference> {
  let mut map: HashMap<String, FileReference> = HashMap::new();
  if let Some(m) = release_manifest {
    for f in &m.files {
      map.insert(f.name.clone(), FileReference { file: f.clone(), patch: None });
    }
  }
  for patch in crate::utils::patch_markers::read_installed_patches(installed_path) {
    let path = installed_path.join(".patches").join(format!("{}.manifest.json", patch.name));
    let Ok(content) = std::fs::read_to_string(&path) else {
      continue;
    };
    let Ok(m) = serde_json::from_str::<ReleaseManifest>(&content) else {
      log::warn!("Cannot parse patch manifest {:?}", path);
      continue;
    };
    for f in &m.files {
      map.insert(f.name.clone(), FileReference { file: f.clone(), patch: Some(patch.name.clone()) });
    }
  }
  map
}

#[tauri::command]
pub async fn verify_installed_version(
  app: tauri::AppHandle,
  channel_map: tauri::State<'_, CancelMap>,
  upload_cancel_map: tauri::State<'_, crate::handlers::upload_v2::UploadCancelMap>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  game_tracker: tauri::State<'_, Arc<crate::service::game_tracker::GameTracker>>,
  versionName: String,
) -> Result<VerifyReport, String> {
  // One verify per version.
  let verify_key = format!("verify:{}", &versionName);
  if crate::utils::locks::lock(&channel_map).contains_key(&verify_key) {
    return Err(ERR_VERIFY_ALREADY_RUNNING.to_string());
  }
  // Refuse while a download of this version is active.
  if crate::utils::locks::lock(&channel_map).contains_key(&versionName) {
    return Err(ERR_DOWNLOAD_ALREADY_RUNNING.to_string());
  }
  // Refuse while a patch install of this version is running (same files).
  let patch_key = format!("patch-install:{}", &versionName);
  if crate::utils::locks::lock(&upload_cancel_map).contains_key(&patch_key) {
    return Err("PATCH_INSTALL_IS_RUNNING".to_string());
  }
  // Refuse while the game is running (files may be locked / changing).
  if game_tracker.status().await.running {
    return Err("GAME_IS_RUNNING".to_string());
  }

  let (installed_path, release_manifest) = {
    let cfg = app_config.lock().await;
    let v = cfg
      .installed_versions
      .values()
      .find(|v| v.name == versionName)
      .ok_or_else(|| format!("Version '{}' not found in installed_versions", &versionName))?;
    (v.installed_path.clone(), v.manifest.clone())
  };

  let reference = build_file_reference(release_manifest.as_ref(), Path::new(&installed_path));
  let raw_files: Vec<&ReleaseManifestFile> = reference.values().map(|r| &r.file).filter(|f| f.kind == ManifestFileKind::Raw).collect();
  let total_files = raw_files.len() as u32;
  let total_bytes: u64 = raw_files.iter().map(|f| f.size).sum();

  let mut report = VerifyReport {
    checked: 0,
    ok: 0,
    missing: vec![],
    size_mismatch: vec![],
    hash_mismatch: vec![],
    skipped_no_hash: 0,
  };

  let cancel = crate::handlers::start_download_version::CancelHandle::new();
  {
    crate::utils::locks::lock(&channel_map).insert(verify_key.clone(), cancel.clone());
  }
  scopeguard::defer! {
    crate::utils::locks::lock(&channel_map).remove(&verify_key);
  };

  // Hashing runs in spawn_blocking; the handle's flag lets a cancel abort a
  // long hash.  It is the SAME flag the cancel command sets, so a cancel that
  // arrives before any receiver exists is no longer lost.
  let cancel_flag = cancel.flag.clone();

  let mut done_files: u32 = 0;
  let mut done_bytes: u64 = 0;

  for file in raw_files {
    if cancel_flag.load(Ordering::Relaxed) {
      return Err(ERR_USER_CANCELLED.to_string());
    }

    let rel = file.target.as_deref().filter(|t| !t.is_empty()).unwrap_or(&file.name).replace('\\', "/");
    if crate::utils::paths::assert_relative_target(&rel).is_err() {
      // Such an entry cannot be checked at all — but silently skipping it left
      // the progress short of 100% and the file invisible in the report, so
      // the player read a broken install as a clean one. Count it and list it
      // among the broken files instead.
      log::warn!("verify: entry with invalid target '{}' cannot be checked", &rel);
      report.checked += 1;
      report.missing.push(if rel != file.name { format!("{} ({})", &file.name, &rel) } else { file.name.clone() });
      done_files += 1;
      done_bytes += file.size;
      continue;
    }
    let path = Path::new(&installed_path).join(&rel);

    report.checked += 1;

    let label = if rel != file.name { format!("{} ({})", &file.name, &rel) } else { file.name.clone() };
    let _ = app.emit(
      "verify-installed-progress",
      VerifyInstalledProgress {
        version_name: versionName.clone(),
        file: label.clone(),
        done_files,
        total_files,
        done_bytes,
        total_bytes,
      },
    );

    let meta = match tokio::fs::metadata(&path).await {
      Ok(m) => m,
      Err(_) => {
        report.missing.push(label);
        done_files += 1;
        continue;
      }
    };

    if meta.len() != file.size {
      report.size_mismatch.push(label);
      done_files += 1;
      done_bytes += file.size;
      continue;
    }

    let Some(expected) = file.sha256.as_deref() else {
      // Old manifest without hashes: the size check is all we can do.
      report.skipped_no_hash += 1;
      report.ok += 1;
      done_files += 1;
      done_bytes += file.size;
      continue;
    };

    let p = path.clone();
    let flag = cancel_flag.clone();
    let actual = tokio::task::spawn_blocking(move || crate::utils::hash::sha256_file(&p, None, Some(&flag))).await;
    match actual {
      Ok(Ok(hash)) if hash.eq_ignore_ascii_case(expected) => {
        report.ok += 1;
      }
      Ok(Ok(hash)) => {
        log::warn!("verify: hash mismatch for '{}': expected {}, got {}", &label, expected, hash);
        report.hash_mismatch.push(label.clone());
      }
      Ok(Err(_)) => {
        // sha256_file() returns Err both when the cancel flag aborted it and
        // on a genuine read error (locked/unreadable file) — only the former
        // is a real user cancel; the latter must be reported as a mismatch,
        // not silently turned into USER_CANCELLED.
        if cancel_flag.load(Ordering::Relaxed) {
          return Err(ERR_USER_CANCELLED.to_string());
        }
        log::warn!("verify: hashing of '{}' errored (file locked or unreadable)", &label);
        report.hash_mismatch.push(label.clone());
      }
      Err(e) => {
        log::error!("verify: hashing task failed: {}", e);
        report.hash_mismatch.push(label.clone());
      }
    }

    done_files += 1;
    done_bytes += file.size;

    let _ = app.emit(
      "verify-installed-progress",
      VerifyInstalledProgress {
        version_name: versionName.clone(),
        file: label,
        done_files,
        total_files,
        done_bytes,
        total_bytes,
      },
    );
  }

  log::info!(
    "verify_installed_version '{}': checked={}, ok={}, missing={}, size_mismatch={}, hash_mismatch={}, skipped_no_hash={}",
    &versionName, report.checked, report.ok, report.missing.len(), report.size_mismatch.len(), report.hash_mismatch.len(), report.skipped_no_hash
  );

  Ok(report)
}

#[tauri::command]
pub async fn cancel_verify_installed_version(channel_map: tauri::State<'_, CancelMap>, versionName: String) -> Result<(), String> {
  let key = format!("verify:{}", &versionName);
  let handle = crate::utils::locks::lock(&channel_map).get(&key).cloned();
  if let Some(handle) = handle {
    handle.cancel();
  }
  Ok(())
}

/// Repair an installed version: re-download ONLY the broken/missing files
/// through the regular download pipeline (hash-verified, raw files copied
/// back into place). Called by the "Repair" button of the verify report.
#[tauri::command]
pub async fn start_repair_version(
  app: tauri::AppHandle,
  channel_map: tauri::State<'_, CancelMap>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  service_files: tauri::State<'_, Arc<ServiceFiles>>,
  service_unpack: tauri::State<'_, Arc<ServiceUnpacker>>,
  versionName: String,
  files: Vec<String>,
) -> Result<(), String> {
  if files.is_empty() {
    return Ok(());
  }
  if crate::utils::locks::lock(&channel_map).contains_key(&versionName) {
    return Err(ERR_DOWNLOAD_ALREADY_RUNNING.to_string());
  }

  let cancel = crate::handlers::start_download_version::CancelHandle::new();
  let cancel_flag_for_guard = cancel.flag.clone();
  {
    crate::utils::locks::lock(&channel_map).insert(versionName.clone(), cancel.clone());
  }
  scopeguard::defer! {
    let mut map = crate::utils::locks::lock(&channel_map);
    if map.get(&versionName).is_some_and(|h| Arc::ptr_eq(&h.flag, &cancel_flag_for_guard)) {
      map.remove(&versionName);
    }
  };

  let (version_id, version_path, installed_path, release_manifest) = {
    let cfg = app_config.lock().await;
    let v = cfg
      .installed_versions
      .values()
      .find(|v| v.name == versionName)
      .ok_or_else(|| format!("Version '{}' not found in installed_versions", &versionName))?;
    (v.id, v.path.clone(), v.installed_path.clone(), v.manifest.clone())
  };

  // The reference (kind/target/sha256) may come from a patch manifest.
  let reference = build_file_reference(release_manifest.as_ref(), Path::new(&installed_path));

  // Download links are kept per source: the release assets on one side, every
  // patch's assets on the other. The link MUST match the manifest the hash
  // came from — a file replaced by a patch lives in the updates repo, and the
  // release copy would fail the patch hash on every attempt.
  let release = {
    let service_guard = service.lock().await;
    service_guard
      .get_main_release(&versionName)
      .await
      .map_err(|e| format!("Failed to get main release files: {}", e))?
  };
  let release_links: HashMap<String, String> = release.assets.into_iter().map(|a| (a.name, a.download_link)).collect();

  let api_client = {
    let service_guard = service.lock().await;
    service_guard.api_client.clone()
  };
  // `patch_links[tag][file]` — the asset of that exact patch; `any_patch_links`
  // is the last-patch-wins fallback for a tag the index no longer lists.
  let mut patch_links: HashMap<String, HashMap<String, String>> = HashMap::new();
  let mut any_patch_links: HashMap<String, String> = HashMap::new();
  if let Ok(api) = api_client.current_provider() {
    if let Ok(index) = crate::service::index::load_index(api.id()).await {
      if let Some(entry) = index.releases.iter().find(|r| r.name == versionName || r.path == versionName) {
        for patch in &entry.patches {
          let per_patch = patch_links.entry(patch.tag.clone()).or_default();
          for asset in &patch.assets {
            per_patch.insert(asset.name.clone(), asset.url.clone());
            any_patch_links.insert(asset.name.clone(), asset.url.clone());
          }
        }
      }
    }
  }

  // Pick the link that belongs to the manifest the reference entry came from.
  let resolve_link = |name: &str, source: Option<&str>| -> Option<String> {
    let Some(tag) = source else {
      return release_links.get(name).cloned();
    };
    if let Some(url) = patch_links.get(tag).and_then(|m| m.get(name)) {
      return Some(url.clone());
    }
    if let Some(url) = any_patch_links.get(name) {
      log::warn!("repair: patch '{}' is not in the index, taking '{}' from a later patch", tag, name);
      return Some(url.clone());
    }
    log::warn!("repair: no patch asset for '{}' (patch '{}'), falling back to the release copy", name, tag);
    release_links.get(name).cloned()
  };

  // Temp download dir next to the install dir; removed by the usual
  // download-unpack-version cleanup after the repair finishes.
  let install_dir = PathBuf::from(&installed_path);
  let folder = install_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| versionName.clone());
  let download_path = install_dir
    .parent()
    .map(|p| p.join(format!("{}_repair_data", folder)))
    .unwrap_or_else(|| PathBuf::from(format!("{}_repair_data", installed_path)));
  let _ = std::fs::create_dir_all(&download_path);

  let mut version = VersionProgress {
    id: version_id,
    name: versionName.clone(),
    path: version_path,
    installed_path: installed_path.clone(),
    download_path: download_path.to_string_lossy().into_owned(),
    is_downloaded: false,
    files: HashMap::new(),
    downloaded_files_cnt: 0,
    total_file_count: files.len() as u32,
    manifest: release_manifest.clone(),
  };

  for name in &files {
    let reference_entry = reference.get(name);
    let Some(link) = resolve_link(name, reference_entry.and_then(|r| r.patch.as_deref())) else {
      return Err(format!("No download link found for file '{}'", name));
    };
    if let Some(tag) = reference_entry.and_then(|r| r.patch.as_deref()) {
      log::info!("repair: '{}' is verified against patch '{}' — downloading the patched copy", name, tag);
    }
    let entry = reference_entry.map(|r| &r.file);
    let total_size = entry.map(|e| e.size).unwrap_or(0);
    let mut fp = FileProgress {
      id: name.clone(),
      download_link: link.clone(),
      name: name.clone(),
      is_downloaded: false,
      is_unpacked: false,
      size: 0,
      total_size,
      sha256: entry.and_then(|e| e.sha256.clone()),
      kind: entry.map(|e| e.kind).unwrap_or(ManifestFileKind::Raw),
      target: entry.and_then(|e| e.target.clone()),
      net_retries: 0,
      verify_retries: 0,
      last_error: None,
    };
    if fp.total_size == 0 {
      // Fall back to a HEAD request when the manifest entry is missing.
      if let Ok(api) = api_client.current_provider() {
        if let Ok(size) = api.get_file_content_size(&link).await {
          fp.total_size = size;
        }
      }
    }
    version.files.insert(name.clone(), fp);
  }

  let files_to_download: Vec<FileProgress> = version.files.values().cloned().collect();

  let _ = app.emit(
    "download-version",
    crate::handlers::dto::DownloadProgress {
      version_name: versionName.clone(),
      status: crate::handlers::dto::DownloadStatus::DownloadFiles,
      file: String::new(),
      progress: 0.0,
      downloaded_files_cnt: 0,
      total_file_count: version.total_file_count,
    },
  );

  {
    let mut cfg = app_config.lock().await;
    cfg.progress_download.insert(versionName.clone(), version.clone());
    let _ = cfg.save();
  }

  if cancel.is_cancelled() {
    return Err(ERR_USER_CANCELLED.to_string());
  }

  crate::service::download_worker::run_version_pipeline(
    &app,
    &app_config.inner().clone(),
    &service_files.inner().clone(),
    &service_unpack.inner().clone(),
    api_client,
    &version,
    files_to_download,
    Vec::new(),
    cancel,
  )
  .await
}
