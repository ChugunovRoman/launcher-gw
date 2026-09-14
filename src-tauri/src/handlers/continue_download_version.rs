use crate::{
  configs::AppConfig::{AppConfig, FileProgress},
  consts,
  handlers::{
    dto::{DownlaodFileStat, DownloadProgress, DownloadStatus, ManifestFileKind},
    start_download_version::{enrich_file_progress, CancelMap},
  },
  service::{download_worker::VerifyResult, files::ServiceFiles, get_release::ServiceGetRelease, main::Service, unpack::ServiceUnpacker},
};
use std::{cmp::Reverse, path::{Path, PathBuf}, sync::Arc};
use tauri::Emitter;
use tokio::sync::Mutex;

/// Delete a stale payload together with its `.part` sidecar (D5).
///
/// The resume offset the worker uses lives ONLY in the sidecar
/// (`download_worker.rs:298`), and the sidecar is rewritten with the current
/// on-disk size BEFORE reconciliation runs.  Resetting progress for a changed
/// file without removing both would make the worker append the new revision
/// after the tail of the old one.
async fn remove_stale_payload(download_dir: &Path, name: &str) {
  if let Ok(path) = crate::utils::paths::safe_download_join(download_dir, name) {
    let _ = tokio::fs::remove_file(&path).await;
  }
  if let Ok(path) = crate::utils::paths::safe_download_join(download_dir, &format!("{}.part", name)) {
    let _ = tokio::fs::remove_file(&path).await;
  }
}

/// Push a file into a pipeline queue unless it is already queued.
fn queue_once(queue: &mut Vec<FileProgress>, file: &FileProgress) {
  if !queue.iter().any(|f| f.name == file.name) {
    queue.push(file.clone());
  }
}

/// Re-sync every queued task with the reconciled `version.files` (D3).
///
/// The worker reads metadata from the TASK, not from the config
/// (`download_worker.rs:310`, `:325`), so a task built during the first pass
/// keeps the stale `total_size` / `sha256` / `download_link` / `target` even
/// after reconciliation updated the config entry.  Tasks whose file
/// disappeared from the release — or which no longer satisfy `keep` — are
/// dropped; duplicates are collapsed.
fn sync_queue_with_version(
  queue: &mut Vec<FileProgress>,
  version: &crate::configs::AppConfig::VersionProgress,
  keep: impl Fn(&FileProgress) -> bool,
) {
  let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
  queue.retain(|task| match version.files.get(&task.name) {
    Some(vf) => keep(vf) && seen.insert(task.name.clone()),
    None => false,
  });
  for task in queue.iter_mut() {
    if let Some(vf) = version.files.get(&task.name) {
      task.id = vf.id.clone();
      task.download_link = vf.download_link.clone();
      task.total_size = vf.total_size;
      task.size = vf.size;
      task.sha256 = vf.sha256.clone();
      task.kind = vf.kind;
      task.target = vf.target.clone();
      task.is_downloaded = vf.is_downloaded;
      task.is_unpacked = vf.is_unpacked;
      task.net_retries = vf.net_retries;
      task.verify_retries = vf.verify_retries;
      task.last_error = vf.last_error.clone();
    }
  }
}

/// Reconcile saved download progress with an already fetched release/manifest.
///
/// Split out of `reconcile_manifest` so the logic is unit-testable without a
/// live provider. Adds new files, removes disappeared ones, resets progress
/// for files whose size or sha changed, refreshes server metadata for all of
/// them and refuses to touch anything when the server list looks bogus.
async fn reconcile_with_release(
  version: &mut crate::configs::AppConfig::VersionProgress,
  files_to_download: &mut Vec<FileProgress>,
  files_to_postprocess: &mut Vec<FileProgress>,
  assets: &[crate::providers::dto::ReleaseAssetGit],
  manifest: Option<&crate::handlers::dto::ReleaseManifest>,
  log_tag: &str,
) -> Result<(), String> {
  // D8: an index published before the assets were uploaded (or an API that
  // returned an empty list) must NOT be treated as "everything was deleted":
  // step 2 would drop every file, `total_file_count` would become 0 and
  // `finalize_download` (0 >= 0) would mark the version as downloaded.
  if assets.is_empty() {
    log::error!("{}: server release '{}' carries no assets — refusing to reconcile", log_tag, &version.name);
    return Err(consts::ERR_RELEASE_NO_ASSETS.to_string());
  }
  let saved_cnt = version.files.len();
  if saved_cnt > 0 && assets.len() * 2 < saved_cnt {
    log::error!(
      "{}: server release '{}' lists only {} assets against {} saved files — refusing to reconcile",
      log_tag, &version.name, assets.len(), saved_cnt
    );
    return Err(consts::ERR_RELEASE_ASSETS_SHRUNK.to_string());
  }

  // D7: a transient network failure gives `None` here. Overwriting the stored
  // manifest with it loses `exe_path` and silently disables the integrity
  // check (`verify_install.rs:107`), so keep the saved one instead.
  if let Some(m) = manifest {
    version.manifest = Some(m.clone());
  } else {
    log::warn!("{}: release manifest unavailable, keeping the saved one", log_tag);
  }

  let download_dir = PathBuf::from(&version.download_path);

  // Index current server assets by name for fast lookup.
  let names_in_manifest: std::collections::HashSet<&str> =
    assets.iter().map(|a| a.name.as_str()).collect();

  let mut added = 0u32;
  let mut removed = 0u32;
  let mut reset = 0u32;
  let mut relinked = 0u32;
  let mut unblocked = 0u32;

  // 1. Add new files and update changed files.
  for asset in assets {
    if let Some(existing) = version.files.get_mut(&asset.name) {
      // D6: the download link is server metadata, not progress. A re-published
      // release usually keeps sizes and hashes but gets new asset ids in the
      // URLs — updating the link only for "changed" files leaves the whole
      // queue pointing at dead links and every file fails on the network.
      if existing.download_link != asset.download_link {
        log::info!("{}: file '{}' got a new download link", log_tag, &asset.name);
        existing.download_link = asset.download_link.clone();
        relinked += 1;
      }

      // File exists in both — check if size or sha changed.
      let size_changed = existing.total_size != asset.size;
      let sha_changed = manifest.and_then(|m| {
        m.files.iter().find(|f| f.name == asset.name).and_then(|f| f.sha256.as_deref())
      }).is_some_and(|sha| existing.sha256.as_deref() != Some(sha));

      // D9: sha/kind/target always come from the current manifest, even when
      // nothing else changed — a manifest fixed ONLY in `target` must reach
      // the saved progress too.
      enrich_file_progress(existing, manifest);

      if size_changed || sha_changed {
        log::info!("{}: file '{}' changed (size: {}, sha: {}), resetting progress", log_tag, &asset.name, size_changed, sha_changed);
        existing.total_size = asset.size;
        existing.is_downloaded = false;
        existing.is_unpacked = false;
        existing.size = 0;
        existing.last_error = None;
        existing.net_retries = 0;
        existing.verify_retries = 0;
        // D5: the old payload and its resume sidecar must go, otherwise the
        // new revision is appended to the tail of the old file.
        remove_stale_payload(&download_dir, &asset.name).await;
        queue_once(files_to_download, existing);
        reset += 1;
      } else if existing.last_error.as_deref() == Some(consts::FILE_ERR_BAD_MANIFEST) {
        // D9: `BAD_MANIFEST` is terminal by design (R8) — only a corrected
        // manifest can clear it, and this is the place where it arrives.
        let target_valid = existing
          .target
          .as_deref()
          .map(|t| crate::utils::paths::assert_relative_target(&t.replace('\\', "/")).is_ok())
          .unwrap_or(true);
        let name_valid = crate::utils::paths::safe_download_join(&download_dir, &existing.name).is_ok();
        if target_valid && name_valid {
          log::info!("{}: file '{}' cleared of BAD_MANIFEST — the new manifest entry is valid", log_tag, &asset.name);
          existing.last_error = None;
          existing.net_retries = 0;
          existing.verify_retries = 0;
          unblocked += 1;
          if existing.is_downloaded && !existing.is_unpacked {
            queue_once(files_to_postprocess, existing);
          } else if !existing.is_downloaded {
            queue_once(files_to_download, existing);
          }
        }
      }
    } else {
      // New file not in saved progress — add it.
      let mut fp = FileProgress {
        id: asset.name.clone(),
        download_link: asset.download_link.clone(),
        name: asset.name.clone(),
        is_downloaded: false,
        is_unpacked: false,
        size: 0,
        total_size: asset.size,
        sha256: None,
        kind: ManifestFileKind::Zip,
        target: None,
        net_retries: 0,
        verify_retries: 0,
        last_error: None,
      };
      enrich_file_progress(&mut fp, manifest);
      log::info!("{}: new file '{}' added from server manifest", log_tag, &asset.name);
      queue_once(files_to_download, &fp);
      version.files.insert(asset.name.clone(), fp);
      added += 1;
    }
  }

  // 2. Remove files no longer present in the server release.
  let saved_names: Vec<String> = version.files.keys().cloned().collect();
  for name in saved_names {
    if !names_in_manifest.contains(name.as_str()) {
      log::info!("{}: file '{}' removed (no longer in server manifest)", log_tag, &name);
      version.files.remove(&name);
      files_to_download.retain(|f| f.name != name);
      files_to_postprocess.retain(|f| f.name != name);
      removed += 1;
    }
  }

  // 3. Update totals.
  version.total_file_count = version.files.len() as u32;
  version.downloaded_files_cnt = version.files.values().filter(|f| f.is_downloaded).count() as u32;

  if added > 0 || removed > 0 || reset > 0 || relinked > 0 || unblocked > 0 {
    log::info!(
      "{}: manifest reconciliation complete — added {}, removed {}, reset {}, relinked {}, unblocked {}",
      log_tag, added, removed, reset, relinked, unblocked
    );
  }

  Ok(())
}

/// Reconcile saved download progress with the current server manifest.
/// Fetches the release and the manifest, then delegates to
/// `reconcile_with_release`.
async fn reconcile_manifest(
  service: &Service,
  version: &mut crate::configs::AppConfig::VersionProgress,
  files_to_download: &mut Vec<FileProgress>,
  files_to_postprocess: &mut Vec<FileProgress>,
  log_tag: &str,
) -> Result<(), String> {
  let release = service
    .get_main_release(&version.name)
    .await
    .map_err(|e| {
      log::warn!("{}: failed to fetch release for manifest reconciliation: {}", log_tag, e);
      e.to_string()
    })?;

  let manifest = service.get_release_manifest(&version.name).await.ok();

  reconcile_with_release(
    version,
    files_to_download,
    files_to_postprocess,
    &release.assets,
    manifest.as_ref(),
    log_tag,
  )
  .await
}

#[tauri::command]
pub async fn continue_download_version(
  app: tauri::AppHandle,
  channel_map: tauri::State<'_, CancelMap>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  service_files: tauri::State<'_, Arc<ServiceFiles>>,
  service_unpack: tauri::State<'_, Arc<ServiceUnpacker>>,
  versionName: String,
) -> Result<(), String> {
  log::info!("Start continue_download_version, version: {:?}", &versionName);

  // 1. Инициализация каналов отмены
  let cancel = crate::handlers::start_download_version::CancelHandle::new();
  let cancel_flag_for_guard = cancel.flag.clone();
  {
    // Check and insert under ONE guard: with two separate locks a second
    // command could pass the check before the first inserted, overwrite the
    // handle, and then the first `defer` (Arc::ptr_eq) would remove nothing —
    // two pipelines writing into the same files (bug 93).
    let mut map = crate::utils::locks::lock(&channel_map);
    if map.contains_key(&versionName) {
      return Err(consts::ERR_DOWNLOAD_ALREADY_RUNNING.to_string());
    }
    map.insert(versionName.clone(), cancel.clone());
  }
  scopeguard::defer! {
    // Remove only if the map still holds OUR handle: otherwise a later command
    // for the same version would lose its cancellation entry.
    let mut map = crate::utils::locks::lock(&channel_map);
    if map.get(&versionName).is_some_and(|h| Arc::ptr_eq(&h.flag, &cancel_flag_for_guard)) {
      map.remove(&versionName);
    }
  };

  // 2. Сбор статистики и подготовка данных
  let mut file_sizes: Vec<DownlaodFileStat> = vec![];
  // Files rejected by `safe_download_join`: reported to the UI after the
  // `download-version-files` event so the failing file is visible (D10).
  let mut bad_manifest_files: Vec<(String, String)> = vec![];

  // Files that finished downloading in an earlier run and only need a resume
  // re-verify (P5/2.3): the sha256 hash is computed OUTSIDE the config lock
  // below — hashing a multi-GB part while holding the global config mutex
  // would stall every other command that reads the config (bug fix).
  struct PendingVerify {
    name: String,
    file_path: PathBuf,
    part_path: PathBuf,
    total_size: u64,
    sha256: Option<String>,
    file_progress: FileProgress,
  }
  let mut pending_verify: Vec<PendingVerify> = vec![];

  let (mut version, mut files_to_download, mut files_to_postprocess) = {
    let mut cfg_guard = app_config.lock().await;

    // Ensure the download dir exists on resume: start_download_version creates it,
    // but continue_download_version previously did NOT, so a post-restart resume
    // could finish "successfully" with no real files on disk, then fail downstream
    // when removing an already-absent download dir (os error 3).
    {
      let version_data = cfg_guard
        .progress_download
        .get(&versionName)
        .ok_or_else(|| "Version not found".to_string())?;
      let download_dir = Path::new(&version_data.download_path);
      if !download_dir.exists() {
        tokio::fs::create_dir_all(download_dir).await.map_err(|e| e.to_string())?;
      }
    }

    let mut to_download = Vec::new();
    let mut to_postprocess = Vec::new();
    let version_data = {
      let version_data = cfg_guard
        .progress_download
        .get_mut(&versionName)
        .ok_or_else(|| "Version not found".to_string())?;

      let mut files_dwn_cnt: u32 = 0;

      for (_, file_progress) in version_data.files.iter_mut() {
        let download_dir = Path::new(&version_data.download_path);
        let file_path = match crate::utils::paths::safe_download_join(download_dir, &file_progress.name) {
          Ok(p) => p,
          Err(e) => {
            // D4/D10: an unsafe remote name is fixable only by a new manifest,
            // so the code must be terminal (R8 semantics) — the previous
            // non-terminal COPY_FAILED made the file fail on every resume and,
            // because of the `continue`, it never reached `file_sizes` either,
            // leaving the version in DOWNLOAD_FAILED with no visible culprit.
            log::error!("unsafe path for '{}': {}", &file_progress.name, e);
            file_progress.last_error = Some(consts::FILE_ERR_BAD_MANIFEST.to_string());
            file_progress.is_downloaded = false;
            file_progress.size = 0;
            file_sizes.push(DownlaodFileStat {
              name: file_progress.name.clone(),
              unpacked: file_progress.is_unpacked,
              size: Some(0),
            });
            bad_manifest_files.push((file_progress.name.clone(), e));
            continue;
          }
        };
        let file_part_path = match crate::utils::paths::safe_download_join(download_dir, &format!("{}.part", &file_progress.name)) {
          Ok(p) => p,
          Err(e) => {
            // D4/D10: an unsafe remote name is fixable only by a new manifest,
            // so the code must be terminal (R8 semantics) — the previous
            // non-terminal COPY_FAILED made the file fail on every resume and,
            // because of the `continue`, it never reached `file_sizes` either,
            // leaving the version in DOWNLOAD_FAILED with no visible culprit.
            log::error!("unsafe path for '{}.part': {}", &file_progress.name, e);
            file_progress.last_error = Some(consts::FILE_ERR_BAD_MANIFEST.to_string());
            file_progress.is_downloaded = false;
            file_progress.size = 0;
            file_sizes.push(DownlaodFileStat {
              name: file_progress.name.clone(),
              unpacked: file_progress.is_unpacked,
              size: Some(0),
            });
            bad_manifest_files.push((file_progress.name.clone(), e));
            continue;
          }
        };

        // ---------------------------------------------------------------
        // P5: the file on disk is the source of truth, the `.part` sidecar
        // is only a hint written before the file was flushed.
        // ---------------------------------------------------------------
        let part_size: Option<u64> = tokio::fs::read_to_string(&file_part_path)
          .await
          .ok()
          .and_then(|s| s.trim().parse::<u64>().ok());
        let file_len: u64 = match tokio::fs::metadata(&file_path).await {
          Ok(meta) => meta.len(),
          Err(_) => 0,
        };

        let current_size = if let Some(part) = part_size {
          if file_len == 0 {
            // Sidecar exists but the file is gone → start over.
            0
          } else if file_len < part {
            // Crash lost bytes after the sidecar write → trust the file.
            file_len
          } else if file_len > part {
            // Extra bytes of unknown quality past the recorded point → cut
            // the file back so the Range resume stays consistent.
            if let Ok(f) = tokio::fs::OpenOptions::new().write(true).open(&file_path).await {
              let _ = f.set_len(part).await;
            }
            part
          } else {
            part
          }
        } else if file_len > 0 && file_progress.is_downloaded {
          file_len
        } else if file_len == 0 && file_progress.is_unpacked {
          // Already post-processed: the archive was removed after unpack.
          file_progress.total_size
        } else {
          0
        };

        file_progress.size = current_size;

        // Reconcile the `.part` sidecar with the decision above.  The download
        // worker reads its resume offset from this file ONLY, so leaving a
        // stale (larger) value here makes it seek past the real end of the
        // file and write the remainder after a hole of zero bytes — a
        // correctly sized but silently corrupt archive whenever the manifest
        // carries no sha256 to catch it.
        if current_size > 0 {
          let _ = tokio::fs::write(&file_part_path, current_size.to_string()).await;
        } else {
          let _ = tokio::fs::remove_file(&file_part_path).await;
        }

        // ---------------------------------------------------------------
        // Retry entries from a previous failed run: reset the counters and
        // re-route by error kind (network/hash → download, unpack/copy →
        // post-process; the archive is still on disk for the latter).
        // ---------------------------------------------------------------
        if let Some(err) = file_progress.last_error.take() {
          file_progress.net_retries = 0;
          file_progress.verify_retries = 0;
          match err.as_str() {
            consts::FILE_ERR_COPY_FAILED => {
              // Validate the target path before retrying — an invalid target
              // will fail every time, so mark it terminal to avoid an infinite
              // retry loop.
              let target_valid = file_progress.target.as_deref()
                .map(|t| crate::utils::paths::assert_relative_target(&t.replace('\\', "/")).is_ok())
                .unwrap_or(true);
              if !target_valid {
                log::error!("File '{}' has invalid target {:?}, marking as terminal error", &file_progress.name, &file_progress.target);
                file_progress.last_error = Some(consts::FILE_ERR_BAD_MANIFEST.to_string());
                file_sizes.push(DownlaodFileStat {
                  name: file_progress.name.clone(),
                  unpacked: file_progress.is_unpacked,
                  size: Some(current_size),
                });
                continue;
              }
              if file_len > 0 {
                file_progress.is_downloaded = true;
                to_postprocess.push(file_progress.clone());
                files_dwn_cnt += 1;
              } else {
                file_progress.is_downloaded = false;
                to_download.push(file_progress.clone());
              }
            }
            consts::FILE_ERR_UNPACK_FAILED => {
              if file_len > 0 {
                file_progress.is_downloaded = true;
                to_postprocess.push(file_progress.clone());
                files_dwn_cnt += 1;
              } else {
                file_progress.is_downloaded = false;
                to_download.push(file_progress.clone());
              }
            }
            consts::FILE_ERR_BAD_MANIFEST => {
              // Terminal: the manifest entry is invalid (e.g. path traversal).
              // Do NOT reset last_error or re-queue — the file will fail every
              // time. Leave the version in error state with a visible reason (R8 fix).
              log::error!("File '{}' has terminal BAD_MANIFEST error, not retrying", &file_progress.name);
              file_progress.last_error = Some(err);
            }
            _ => {
              file_progress.is_downloaded = false;
              to_download.push(file_progress.clone());
            }
          }

          file_sizes.push(DownlaodFileStat {
            name: file_progress.name.clone(),
            unpacked: file_progress.is_unpacked,
            size: Some(current_size),
          });
          continue;
        }

        if current_size >= file_progress.total_size && file_progress.total_size > 0 {
          file_progress.is_downloaded = true;
          files_dwn_cnt += 1;

          // 2.3: a file marked downloaded but not yet post-processed is
          // re-verified before unpacking — it may have been corrupted on
          // disk between sessions. On mismatch it goes back to the queue.
          // The hash itself is computed AFTER the config lock is released
          // (see `pending_verify` below) — this loop only queues it.
          if !file_progress.is_unpacked && file_len > 0 {
            pending_verify.push(PendingVerify {
              name: file_progress.name.clone(),
              file_path: file_path.clone(),
              part_path: file_part_path.clone(),
              total_size: file_progress.total_size,
              sha256: file_progress.sha256.clone(),
              file_progress: file_progress.clone(),
            });
          }
        } else {
          file_progress.is_downloaded = false;
          to_download.push(file_progress.clone());
        }

        file_sizes.push(DownlaodFileStat {
          name: file_progress.name.clone(),
          unpacked: file_progress.is_unpacked,
          size: Some(current_size),
        });
      }

      version_data.downloaded_files_cnt = files_dwn_cnt;
      version_data.clone()
    };

    cfg_guard.save().map_err(|e| e.to_string())?;

    (version_data.clone(), to_download, to_postprocess)
  };
  // ^ config lock released here — the hashing below must not hold it.

  // Phase A.5: reconcile saved progress with the current server manifest.
  // After a release is re-published, new files must be added, disappeared
  // files removed, and files whose size/sha changed must be re-downloaded.
  {
    let svc = service.lock().await;
    if let Err(e) = reconcile_manifest(&svc, &mut version, &mut files_to_download, &mut files_to_postprocess, "continue_download").await {
      log::warn!("Manifest reconciliation failed (continuing with saved progress): {}", e);
    }
  }
  // After reconciliation, re-sync the queues with `version.files` (D3): tasks
  // built during the first pass still carry the OLD total_size / sha256 /
  // download_link / target, and the worker takes its metadata from the task.
  // The same pass drops stale entries: removed files and files whose sha/size
  // changed were reset to "not downloaded" and must not be unpacked from the
  // old archive (R4 fix).
  sync_queue_with_version(&mut files_to_download, &version, |_| true);
  sync_queue_with_version(&mut files_to_postprocess, &version, |vf| vf.is_downloaded && !vf.is_unpacked);
  pending_verify.retain(|pv| {
    version.files.get(&pv.name).is_some_and(|vf| vf.is_downloaded && !vf.is_unpacked)
  });
  // Persist reconciled state so a subsequent resume sees the updated file list.
  {
    let mut cfg_guard = app_config.lock().await;
    cfg_guard.progress_download.insert(version.name.clone(), version.clone());
    let _ = cfg_guard.save();
  }

  // Phase B: resume re-verify (P5/2.3), OUTSIDE the config lock.
  // Hashing every already-downloaded part takes minutes on a large version, so
  // a cancel arriving here must be honoured — previously it was swallowed and
  // the download started anyway, with no way left to stop it.
  let mut regressed_names: Vec<String> = vec![];
  for pv in pending_verify {
    if cancel.is_cancelled() {
      log::info!("continue_download_version: cancelled during resume verification");
      return Err(consts::ERR_USER_CANCELLED.to_string());
    }
    let verify = crate::service::download_worker::verify_downloaded_file(&pv.file_path, pv.total_size, pv.sha256.as_deref()).await;
    match verify {
      Ok(VerifyResult::Ok) | Ok(VerifyResult::Skipped) => {
        files_to_postprocess.push(pv.file_progress);
      }
      Ok(VerifyResult::SizeMismatch { .. }) | Ok(VerifyResult::HashMismatch { .. }) => {
        log::warn!("Resume verify failed for '{}': re-downloading", &pv.name);
        let _ = tokio::fs::remove_file(&pv.file_path).await;
        let _ = tokio::fs::remove_file(&pv.part_path).await;
        let mut fp = pv.file_progress;
        fp.is_downloaded = false;
        fp.size = 0;
        regressed_names.push(fp.name.clone());
        files_to_download.push(fp);
      }
      Err(e) => {
        // I/O error while hashing — re-check the file to decide whether
        // it is still valid.  If the file is gone or the size drifted,
        // re-download; otherwise (genuine transient read error during
        // hashing) keep the file and let it through to post-processing.
        match tokio::fs::metadata(&pv.file_path).await {
          Ok(m) if m.len() == pv.total_size => {
            log::warn!("Resume verify of '{}' errored (kept, size matches): {}", &pv.name, e);
            files_to_postprocess.push(pv.file_progress);
          }
          _ => {
            log::warn!("Resume verify of '{}' errored and file missing/wrong size: re-downloading", &pv.name);
            let _ = tokio::fs::remove_file(&pv.file_path).await;
            let _ = tokio::fs::remove_file(&pv.part_path).await;
            let mut fp = pv.file_progress;
            fp.is_downloaded = false;
            fp.size = 0;
            regressed_names.push(fp.name.clone());
            files_to_download.push(fp);
          }
        }
      }
    }
  }

  if !regressed_names.is_empty() {
    version.downloaded_files_cnt = version.downloaded_files_cnt.saturating_sub(regressed_names.len() as u32);
    let mut cfg_guard = app_config.lock().await;
    if let Some(ver) = cfg_guard.progress_download.get_mut(&versionName) {
      ver.downloaded_files_cnt = version.downloaded_files_cnt;
      for name in &regressed_names {
        if let Some(existing) = ver.files.get_mut(name) {
          existing.is_downloaded = false;
          existing.size = 0;
        }
      }
    }
    let _ = cfg_guard.save();
  }

  // Reflect the reconciled state in the UI snapshot: a file whose progress was
  // reset (changed size/sha) must not keep showing the byte count collected
  // before reconciliation, and a file dropped from the release must not be
  // listed at all.
  file_sizes.retain(|stat| version.files.contains_key(&stat.name));
  for stat in file_sizes.iter_mut() {
    if let Some(vf) = version.files.get(&stat.name) {
      stat.size = Some(vf.size);
      stat.unpacked = vf.is_unpacked;
    }
  }

  // Сортировка для UI (по номеру чанка в расширении)
  file_sizes.sort_by_key(|file| Reverse(file.size));
  files_to_download.sort_by_key(|file| Reverse(file.size));

  let _ = app.emit("download-version-files", (&versionName, &file_sizes));
  // D10: report files rejected by `safe_download_join` AFTER the file list is
  // emitted — the frontend only applies an error to a file it already knows.
  for (name, message) in &bad_manifest_files {
    let _ = app.emit(
      "download-version-file-error",
      crate::handlers::dto::FileErrorPayload {
        version_name: versionName.clone(),
        file: name.clone(),
        code: consts::FILE_ERR_BAD_MANIFEST.to_string(),
        message: message.clone(),
      },
    );
  }
  // Bug E fix: guard against division by zero.
  let progress = if version.total_file_count > 0 {
    (version.downloaded_files_cnt as f32 / version.total_file_count as f32) * 100.0
  } else {
    0.0
  };
  // Отправляем ивент на фронт с сохраненными данными о прогресса после паузы. Это нужна для начальной инициализации UI
  let _ = app.emit(
    "download-version",
    DownloadProgress {
      version_name: version.name.clone(),
      status: DownloadStatus::DownloadFiles,
      file: "".to_string(),
      progress,
      downloaded_files_cnt: version.downloaded_files_cnt,
      total_file_count: version.total_file_count,
    },
  );

  if cancel.is_cancelled() {
    return Err(consts::ERR_USER_CANCELLED.to_string());
  }

  let api_client = service.lock().await.api_client.clone();

  crate::service::download_worker::run_version_pipeline(
    &app,
    &app_config.inner().clone(),
    &service_files.inner().clone(),
    &service_unpack.inner().clone(),
    api_client,
    &version,
    files_to_download,
    files_to_postprocess,
    cancel,
  )
  .await
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::configs::AppConfig::VersionProgress;
  use crate::handlers::dto::{ReleaseManifest, ReleaseManifestFile};
  use crate::providers::dto::{ReleaseAssetGit, ReleasePlatform};
  use std::collections::HashMap;

  fn asset(name: &str, size: u64, link: &str) -> ReleaseAssetGit {
    ReleaseAssetGit {
      name: name.to_string(),
      platform: ReleasePlatform::Windows,
      size,
      download_link: link.to_string(),
    }
  }

  fn file(name: &str, size: u64, link: &str, sha: Option<&str>) -> FileProgress {
    FileProgress {
      id: name.to_string(),
      download_link: link.to_string(),
      name: name.to_string(),
      is_downloaded: false,
      is_unpacked: false,
      size: 0,
      total_size: size,
      sha256: sha.map(|s| s.to_string()),
      kind: ManifestFileKind::Zip,
      target: None,
      net_retries: 0,
      verify_retries: 0,
      last_error: None,
    }
  }

  fn version(download_path: &str, files: Vec<FileProgress>) -> VersionProgress {
    let mut map: HashMap<String, FileProgress> = HashMap::new();
    for f in files {
      map.insert(f.name.clone(), f);
    }
    VersionProgress {
      id: 1,
      name: "v1".to_string(),
      path: String::new(),
      installed_path: String::new(),
      download_path: download_path.to_string(),
      total_file_count: map.len() as u32,
      downloaded_files_cnt: 0,
      is_downloaded: false,
      files: map,
      manifest: None,
    }
  }

  fn manifest_of(entries: Vec<(&str, u64, Option<&str>, Option<&str>)>) -> ReleaseManifest {
    ReleaseManifest {
      schema: 2,
      files: entries
        .into_iter()
        .map(|(name, size, sha, target)| ReleaseManifestFile {
          name: name.to_string(),
          size,
          sha256: sha.map(|s| s.to_string()),
          kind: ManifestFileKind::Zip,
          target: target.map(|t| t.to_string()),
        })
        .collect(),
      exe_path: Some("bin/xrEngine.exe".to_string()),
      ..Default::default()
    }
  }

  fn tmp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gw_reconcile_{}_{}", tag, std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
  }

  /// D6: a re-published release keeps sizes and hashes but hands out new asset
  /// URLs — the link must be refreshed for EVERY file, and the already queued
  /// task must pick the new link up (D3).
  #[tokio::test]
  async fn reconcile_refreshes_download_link_without_size_or_sha_change() {
    let dir = tmp_dir("relink");
    let mut version = version(
      dir.to_str().unwrap(),
      vec![file("data0.zip", 100, "https://old/1", Some("aa"))],
    );
    let mut to_download = vec![file("data0.zip", 100, "https://old/1", Some("aa"))];
    let mut to_postprocess: Vec<FileProgress> = vec![];
    let manifest = manifest_of(vec![("data0.zip", 100, Some("aa"), None)]);

    reconcile_with_release(
      &mut version,
      &mut to_download,
      &mut to_postprocess,
      &[asset("data0.zip", 100, "https://new/9")],
      Some(&manifest),
      "test",
    )
    .await
    .expect("reconcile must succeed");

    assert_eq!(version.files["data0.zip"].download_link, "https://new/9");
    // The queue still holds the stale clone until it is re-synced (D3).
    sync_queue_with_version(&mut to_download, &version, |_| true);
    assert_eq!(to_download.len(), 1);
    assert_eq!(to_download[0].download_link, "https://new/9");
  }

  /// D8: an empty asset list must not wipe the saved file list (which would
  /// make `finalize_download` mark an empty version as fully downloaded).
  #[tokio::test]
  async fn reconcile_rejects_empty_asset_list() {
    let dir = tmp_dir("empty");
    let mut version = version(
      dir.to_str().unwrap(),
      vec![file("data0.zip", 100, "https://old/1", Some("aa"))],
    );
    let mut to_download: Vec<FileProgress> = vec![];
    let mut to_postprocess: Vec<FileProgress> = vec![];

    let err = reconcile_with_release(&mut version, &mut to_download, &mut to_postprocess, &[], None, "test")
      .await
      .expect_err("empty asset list must be rejected");

    assert_eq!(err, consts::ERR_RELEASE_NO_ASSETS);
    assert_eq!(version.files.len(), 1);
    assert_eq!(version.total_file_count, 1);
  }

  /// D8: a drastically shrunk asset list is treated as a server glitch too.
  #[tokio::test]
  async fn reconcile_rejects_shrunk_asset_list() {
    let dir = tmp_dir("shrunk");
    let mut version = version(
      dir.to_str().unwrap(),
      vec![
        file("data0.zip", 100, "https://old/0", None),
        file("data1.zip", 100, "https://old/1", None),
        file("data2.zip", 100, "https://old/2", None),
        file("data3.zip", 100, "https://old/3", None),
      ],
    );
    let mut to_download: Vec<FileProgress> = vec![];
    let mut to_postprocess: Vec<FileProgress> = vec![];

    let err = reconcile_with_release(
      &mut version,
      &mut to_download,
      &mut to_postprocess,
      &[asset("data0.zip", 100, "https://new/0")],
      None,
      "test",
    )
    .await
    .expect_err("shrunk asset list must be rejected");

    assert_eq!(err, consts::ERR_RELEASE_ASSETS_SHRUNK);
    assert_eq!(version.files.len(), 4);
  }

  /// D7: a transient manifest fetch failure must not erase the stored manifest.
  #[tokio::test]
  async fn reconcile_keeps_saved_manifest_when_fetch_failed() {
    let dir = tmp_dir("manifest");
    let mut version = version(
      dir.to_str().unwrap(),
      vec![file("data0.zip", 100, "https://old/1", Some("aa"))],
    );
    version.manifest = Some(manifest_of(vec![("data0.zip", 100, Some("aa"), None)]));
    let mut to_download: Vec<FileProgress> = vec![];
    let mut to_postprocess: Vec<FileProgress> = vec![];

    reconcile_with_release(
      &mut version,
      &mut to_download,
      &mut to_postprocess,
      &[asset("data0.zip", 100, "https://old/1")],
      None,
      "test",
    )
    .await
    .expect("reconcile must succeed");

    let manifest = version.manifest.as_ref().expect("saved manifest must survive");
    assert_eq!(manifest.exe_path.as_deref(), Some("bin/xrEngine.exe"));
  }

  /// D5: a changed file loses both its payload and its `.part` sidecar, so the
  /// worker cannot resume the new revision at the old offset.
  #[tokio::test]
  async fn reconcile_removes_payload_and_part_of_changed_file() {
    let dir = tmp_dir("stale");
    let payload = dir.join("data0.zip");
    let part = dir.join("data0.zip.part");
    std::fs::write(&payload, b"old bytes").unwrap();
    std::fs::write(&part, b"9").unwrap();

    let mut existing = file("data0.zip", 100, "https://old/1", Some("aa"));
    existing.size = 9;
    let mut version = version(dir.to_str().unwrap(), vec![existing]);
    let mut to_download: Vec<FileProgress> = vec![];
    let mut to_postprocess: Vec<FileProgress> = vec![];
    let manifest = manifest_of(vec![("data0.zip", 200, Some("bb"), None)]);

    reconcile_with_release(
      &mut version,
      &mut to_download,
      &mut to_postprocess,
      &[asset("data0.zip", 200, "https://new/1")],
      Some(&manifest),
      "test",
    )
    .await
    .expect("reconcile must succeed");

    assert!(!payload.exists(), "stale payload must be deleted");
    assert!(!part.exists(), "stale .part sidecar must be deleted");
    assert_eq!(version.files["data0.zip"].size, 0);
    assert_eq!(version.files["data0.zip"].sha256.as_deref(), Some("bb"));
    assert_eq!(to_download.len(), 1);
  }

  /// D9: a manifest corrected ONLY in `target` must clear the terminal
  /// BAD_MANIFEST error and put the file back into the pipeline.
  #[tokio::test]
  async fn reconcile_clears_bad_manifest_when_target_fixed() {
    let dir = tmp_dir("badmanifest");
    let mut existing = file("data0.zip", 100, "https://old/1", Some("aa"));
    existing.target = Some("../escape/x.db".to_string());
    existing.last_error = Some(consts::FILE_ERR_BAD_MANIFEST.to_string());
    let mut version = version(dir.to_str().unwrap(), vec![existing]);
    let mut to_download: Vec<FileProgress> = vec![];
    let mut to_postprocess: Vec<FileProgress> = vec![];
    let manifest = manifest_of(vec![("data0.zip", 100, Some("aa"), Some("gamedata/x.db"))]);

    reconcile_with_release(
      &mut version,
      &mut to_download,
      &mut to_postprocess,
      &[asset("data0.zip", 100, "https://old/1")],
      Some(&manifest),
      "test",
    )
    .await
    .expect("reconcile must succeed");

    let fp = &version.files["data0.zip"];
    assert_eq!(fp.target.as_deref(), Some("gamedata/x.db"));
    assert!(fp.last_error.is_none(), "corrected manifest must clear BAD_MANIFEST");
    assert_eq!(to_download.len(), 1, "unblocked file must be queued again");
  }

  /// D3: queued tasks are rebuilt from the reconciled file list — stale entries
  /// are dropped, duplicates collapsed, metadata refreshed.
  #[test]
  fn sync_queue_rewrites_stale_tasks() {
    let mut version = version("C:/downloads", vec![file("data0.zip", 300, "https://new/0", Some("bb"))]);
    if let Some(fp) = version.files.get_mut("data0.zip") {
      fp.target = Some("gamedata/x.db".to_string());
      fp.kind = ManifestFileKind::Raw;
    }
    let mut queue = vec![
      file("data0.zip", 100, "https://old/0", Some("aa")),
      file("data0.zip", 100, "https://old/0", Some("aa")),
      file("gone.zip", 100, "https://old/9", None),
    ];

    sync_queue_with_version(&mut queue, &version, |_| true);

    assert_eq!(queue.len(), 1, "duplicates and removed files must be dropped");
    assert_eq!(queue[0].total_size, 300);
    assert_eq!(queue[0].sha256.as_deref(), Some("bb"));
    assert_eq!(queue[0].download_link, "https://new/0");
    assert_eq!(queue[0].target.as_deref(), Some("gamedata/x.db"));
    assert_eq!(queue[0].kind, ManifestFileKind::Raw);
  }
}
