use crate::consts::{self, MANIFEST_NAME};
use crate::handlers::dto::{CompressProgressPayload, ManifestFileKind, PatchMeta, ReleaseManifest, ReleaseManifestFile};
use crate::utils::CountingWriter::CountingWriter;
use anyhow::Result;
use globset::{GlobBuilder, GlobSetBuilder};
use serde::Serialize;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use tauri::Emitter;
use walkdir::WalkDir;
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

/// What the Pack view gets back from a finished pack run.
///
/// A pack can succeed and still be incomplete: unreadable source files are
/// skipped (R2) and only logged. Their names are reported here so the UI can
/// warn the maintainer instead of showing a plain "done" (item 33).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackResult {
  /// Source files (relative paths) left out of the pack because they could not
  /// be opened. Empty on a fully successful run.
  pub skipped_files: Vec<String>,
  /// Same semantics as `ReleaseManifest::total_files_count` — the number of
  /// source files a player ends up with after unpacking.
  pub total_files_count: u32,
}

#[tauri::command]
pub async fn create_split_archives(
  app: tauri::AppHandle,
  sourceDir: String,
  targetPath: String,
  chunkSize: u64,
  excludePatterns: Vec<String>,
  exePath: Option<String>,
) -> Result<PackResult, String> {
  let (manifest, skipped_files) =
    pack_split_archives_reported(&app, sourceDir, targetPath, chunkSize, excludePatterns, exePath, None, Vec::new()).await?;

  Ok(PackResult {
    skipped_files,
    total_files_count: manifest.total_files_count,
  })
}

/// Builds split `data{N}.zip` archives (zip+zstd) from `sourceDir` into
/// `targetPath` and writes `manifest.json` next to them. Shared by the Pack
/// view (full releases, `patch_meta = None`) and patch uploads
/// (`patch_meta = Some(..)` adds patch fields into the manifest).
///
/// `extra_raw_files` — (src, target) pairs copied as-is into the pack dir and
/// recorded with `kind = Raw` (dev/test hook for future `.db` archives; not
/// exposed in the Pack UI).
pub async fn pack_split_archives(
  app: &tauri::AppHandle,
  sourceDir: String,
  targetPath: String,
  chunkSize: u64,
  excludePatterns: Vec<String>,
  exePath: Option<String>,
  patch_meta: Option<PatchMeta>,
  extra_raw_files: Vec<(PathBuf, String)>,
) -> Result<ReleaseManifest, String> {
  pack_split_archives_reported(app, sourceDir, targetPath, chunkSize, excludePatterns, exePath, patch_meta, extra_raw_files)
    .await
    .map(|(manifest, _)| manifest)
}

/// Same as [`pack_split_archives`], but also returns the names of source files
/// that were skipped as unreadable (item 33 — the Pack view shows them).
async fn pack_split_archives_reported(
  app: &tauri::AppHandle,
  sourceDir: String,
  targetPath: String,
  chunkSize: u64,
  excludePatterns: Vec<String>,
  exePath: Option<String>,
  patch_meta: Option<PatchMeta>,
  extra_raw_files: Vec<(PathBuf, String)>,
) -> Result<(ReleaseManifest, Vec<String>), String> {
  // Zip+zstd packing of tens of GB is pure sync CPU/IO — run it on the
  // blocking pool so it does not stall the async runtime (and every IPC
  // command with it) for minutes.
  let app = app.clone();
  tokio::task::spawn_blocking(move || {
    pack_split_archives_blocking(&app, sourceDir, targetPath, chunkSize, excludePatterns, exePath, patch_meta, extra_raw_files)
  })
  .await
  .map_err(|e| e.to_string())?
}

/// Close a finished archive: hash it and push its manifest entry, or drop it
/// when nothing was written into it.
///
/// `entries_written` is the number of files stored in this part. Zero means a
/// rollover created `data<N>.zip` and every file that followed was skipped as
/// unreadable — such a part must not reach the manifest, otherwise every client
/// downloads an empty archive for nothing (item 88). The empty file is removed
/// from the pack dir as well.
///
/// The hash is computed by re-reading the file: ZipWriter seeks backwards
/// inside the archive, so a streaming hash over the written bytes is wrong.
fn close_archive(
  out_dir: &Path,
  archive_name: String,
  entries_written: u32,
  manifest: &mut ReleaseManifest,
  compressed_size: &mut u64,
) -> Result<(), String> {
  let file_path = out_dir.join(&archive_name);

  if entries_written == 0 {
    log::warn!("Archive {} got no entries — discarding it instead of shipping an empty part", &archive_name);
    if let Err(e) = fs::remove_file(&file_path) {
      log::warn!("Failed to remove empty archive {:?}: {}", &file_path, e);
    }
    return Ok(());
  }

  let sha = crate::utils::hash::sha256_file(&file_path, None, None).map_err(|e| e.to_string())?;
  let size = file_path.metadata().map_err(|e| e.to_string())?.len();
  *compressed_size += size;
  log::info!("Packed {}: {} bytes, sha256 {}", &archive_name, size, &sha);
  manifest.files.push(ReleaseManifestFile {
    name: archive_name,
    size,
    sha256: Some(sha),
    kind: ManifestFileKind::Zip,
    target: None,
  });
  Ok(())
}

/// Progress event emitted right before an archive is hashed.
fn emit_hashing_progress(app: &tauri::AppHandle, archive_name: &str, total_size: u64, processed_size: u64) {
  let percentage = if total_size > 0 { (processed_size as f64 / total_size as f64) * 100.0 } else { 0.0 };
  let _ = app.emit(
    "packing-progress",
    CompressProgressPayload {
      status: 2,
      current_file: archive_name.to_owned(),
      total_size,
      processed_size,
      percentage,
    },
  );
}

/// True for `data<N>.zip` names produced by a previous pack run.
fn is_pack_archive_name(name: &str) -> bool {
  name.len() > "data.zip".len() && name.starts_with("data") && name.ends_with(".zip") && name[4..name.len() - 4].parse::<u32>().is_ok()
}

/// Remove the output of a previous pack run from `out_dir`.
///
/// Besides `data<N>.zip` and `manifest.json` this also deletes every raw file
/// the previous manifest lists (`kind = Raw`): those are named after their
/// target, survived the old cleanup and shipped to players as garbage (item 72).
///
/// Must be called only after the input has been validated and the source file
/// list collected — a failed validation must not destroy a finished pack
/// (item 80).
fn cleanup_previous_pack(out_dir: &Path) {
  let manifest_path = out_dir.join(MANIFEST_NAME);
  let mut stale_raw: HashSet<String> = HashSet::new();

  match fs::read_to_string(&manifest_path) {
    Ok(text) => match serde_json::from_str::<ReleaseManifest>(&text) {
      Ok(prev) => {
        for file in prev.files.iter().filter(|f| matches!(f.kind, ManifestFileKind::Raw)) {
          // Manifest names are flat file names; go through file_name() anyway so
          // a hand-edited manifest cannot point the cleanup outside out_dir.
          if let Some(name) = Path::new(&file.name).file_name().and_then(|n| n.to_str()) {
            stale_raw.insert(name.to_owned());
          }
        }
      }
      Err(e) => log::warn!("Previous {} is not readable ({}), raw leftovers are not cleaned", MANIFEST_NAME, e),
    },
    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
    Err(e) => log::warn!("Cannot read previous {:?}: {}", &manifest_path, e),
  }

  let entries = match fs::read_dir(out_dir) {
    Ok(entries) => entries,
    Err(e) => {
      log::warn!("Cannot list target dir {:?}: {}", out_dir, e);
      return;
    }
  };

  for entry in entries.flatten() {
    let name = entry.file_name();
    let name_str = name.to_string_lossy();
    let is_stale = is_pack_archive_name(&name_str) || name_str == MANIFEST_NAME || stale_raw.contains(name_str.as_ref());
    if !is_stale {
      continue;
    }
    if entry.path().is_dir() {
      continue;
    }
    if let Err(e) = fs::remove_file(entry.path()) {
      log::warn!("Failed to remove stale file {:?}: {}", entry.path(), e);
    }
  }
}

fn pack_split_archives_blocking(
  app: &tauri::AppHandle,
  sourceDir: String,
  targetPath: String,
  chunkSize: u64,
  excludePatterns: Vec<String>,
  exePath: Option<String>,
  patch_meta: Option<PatchMeta>,
  extra_raw_files: Vec<(PathBuf, String)>,
) -> Result<(ReleaseManifest, Vec<String>), String> {
  let src_dir = Path::new(&sourceDir);
  let out_dir = Path::new(&targetPath);
  let max_size = chunkSize * 1024 * 1024;

  // Input validation happens BEFORE the target dir is touched: pointing the
  // packer at the wrong source folder used to wipe the previous finished pack
  // and only then report "No files found" (item 80).
  if !src_dir.is_dir() {
    return Err(format!("{}: {}", consts::PACK_ERR_SOURCE_NOT_FOUND, &sourceDir));
  }
  if chunkSize == 0 {
    return Err(consts::PACK_ERR_INVALID_CHUNK_SIZE.to_owned());
  }

  let mut builder = GlobSetBuilder::new();
  for pattern in excludePatterns {
    builder.add(
      GlobBuilder::new(&pattern)
        .case_insensitive(true)
        .build()
        .map_err(|e| e.to_string())?,
    );
  }
  let setter = builder.build().map_err(|e| e.to_string())?;

  // 1. Сначала считаем общий размер всех файлов
  let mut total_size = 0;
  let mut all_files = Vec::new();

  let _ = app.emit(
    "packing-progress",
    CompressProgressPayload {
      status: 0,
      current_file: "".to_owned(),
      total_size,
      processed_size: 0,
      percentage: 0.,
    },
  );

  log::debug!("Start compress. Searching files in: {}", &sourceDir);
  // Use filter_entry to prune entire excluded subtrees (e.g. "installer",
  // "Compressor") instead of visiting every file and skipping with `continue`.
  let walker = WalkDir::new(src_dir).follow_links(true).into_iter().filter_entry(|e| {
    match e.path().strip_prefix(src_dir) {
      Ok(rel) => !setter.is_match(rel),
      Err(_) => true,
    }
  });
  for entry in walker {
    // A swallowed walk error (no permissions, too long a path, a symlink loop —
    // follow_links is on) silently drops files from both the archives and the
    // manifest, and the client trusts the manifest. An incomplete release is
    // worse than a refused one, so log and abort (items 75 and 84).
    let entry = match entry {
      Ok(entry) => entry,
      Err(e) => {
        let path = e.path().map(|p| p.display().to_string()).unwrap_or_else(|| sourceDir.clone());
        log::warn!("Directory walk failed at '{}': {}", &path, e);
        return Err(format!("{}: '{}': {}", consts::PACK_ERR_WALK_FAILED, &path, e));
      }
    };

    let full_path = entry.path();
    let relative_path = full_path.strip_prefix(src_dir).map_err(|e| e.to_string())?;

    if entry.file_type().is_file() {
      let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
      total_size += size;
      all_files.push((full_path.to_path_buf(), relative_path.to_path_buf(), size));
    }
  }
  log::debug!(
    "Search files completed ! all_files: {} total_size: {}",
    &all_files.len(),
    total_size
  );

  if all_files.is_empty() {
    return Err(consts::PACK_ERR_NO_SOURCE_FILES.to_owned());
  }

  // Size validation also runs before the cleanup (item 80): a file that can
  // never fit into a chunk aborts the pack without destroying the old one.
  for (_, entry_name, size) in &all_files {
    if *size > max_size {
      return Err(format!(
        "File '{}' ({} MB) exceeds the chunk size limit ({} MB)",
        entry_name.display(),
        size / (1024 * 1024),
        chunkSize
      ));
    }
  }

  // Same for the raw extras: resolve and validate their flat names up front.
  let mut raw_plan: Vec<(PathBuf, String, String)> = Vec::with_capacity(extra_raw_files.len());
  for (src, target) in extra_raw_files {
    let normalized_target = target.replace('\\', "/");
    crate::utils::paths::assert_relative_target(&normalized_target)
      .map_err(|e| format!("extra_raw_files target '{}': {}", &normalized_target, e))?;

    let flat_name = Path::new(&normalized_target)
      .file_name()
      .and_then(|n| n.to_str())
      .ok_or_else(|| format!("extra_raw_files target has no file name: {}", &normalized_target))?
      .to_string();
    if !src.is_file() {
      return Err(format!("extra_raw_files source is not a file: {:?}", &src));
    }
    raw_plan.push((src, normalized_target, flat_name));
  }

  // Only now the target dir may be created and cleaned.
  fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
  // Clean up the previous (possibly failed) pack so that a new one never ships
  // with leftover sha256 or leftover raw files from an old run.
  cleanup_previous_pack(out_dir);

  let mut manifest = ReleaseManifest {
    // schema 2 = sha256 + kind/target per file (see download-integrity plan).
    schema: 2,
    // `total_files_count` counts the source files a player ends up with after
    // unpacking every pack artifact — files actually stored in the archives
    // plus raw files copied as-is. It is NOT `manifest.files.len()` (that is
    // the number of downloadable artifacts) and not the number of scanned
    // source files (unreadable ones are skipped). Filled in after packing so
    // all three places stay consistent (item 89).
    total_files_count: 0,
    total_size,
    compressed_size: 0,
    files: vec![],
    exe_path: None,
    patch_name: None,
    base_patch: None,
    base_release_tag: None,
    deleted_files: vec![],
    updated_fields: vec![],
  };

  // Optional launcher exe (e.g. Stalker-CoC.exe) recorded in the manifest as a
  // path RELATIVE to sourceDir (packPath). The launcher uses it to start the
  // game directly, bypassing -fsltx / CWD workarounds.
  manifest.exe_path = exePath.as_ref().filter(|s| !s.is_empty()).and_then(|abs| {
    let p = Path::new(abs);
    match p.strip_prefix(Path::new(&sourceDir)) {
      Ok(rel) => Some(rel.to_string_lossy().replace('\\', "/")),
      Err(_) => {
        log::warn!("exe_path '{abs}' is not under packPath '{sourceDir}'; using basename");
        p.file_name().map(|n| n.to_string_lossy().into_owned())
      }
    }
  });

  // 2. Процесс упаковки
  let mut processed_size = 0;
  let mut part_number = 1;
  // Entries actually written into the current `data<N>.zip` — an archive with
  // zero entries must not be hashed and recorded (item 88).
  let mut current_group_entries: u32 = 0;
  // Source files stored in the archives (skipped ones excluded) — the basis of
  // `manifest.total_files_count` (item 89).
  let mut packed_files_count: u32 = 0;
  // Names of files skipped as unreadable, reported to the UI (item 33).
  let mut skipped_files: Vec<String> = Vec::new();
  let mut compressed_size: u64 = 0;

  let shared_written = Arc::new(StdMutex::new(0u64));
  let current_archive_size_ref = Arc::clone(&shared_written);

  // Обновляем функцию создания чанка
  let create_chunk = |path: PathBuf, counter: Arc<StdMutex<u64>>| {
    // Сбрасываем счетчик для нового файла
    if let Ok(mut w) = counter.lock() {
      *w = 0;
    }

    let file = File::create(path).map_err(|e| e.to_string())?;
    let writer = BufWriter::new(file);
    let counting_writer = CountingWriter::new(writer, counter);
    Ok::<ZipWriter<CountingWriter<BufWriter<File>>>, String>(ZipWriter::new(counting_writer))
  };

  // Первый чанк
  let mut zip = create_chunk(out_dir.join(format!("data{}.zip", part_number)), Arc::clone(&shared_written))?;

  // Настройки: для 50ГБ лучше всего Zstd (level 3) или Stored (без сжатия)
  let options: FileOptions<'_, ()> = FileOptions::default()
    .compression_method(CompressionMethod::Zstd)
    .compression_level(Some(3));

  let all_files_count = all_files.len();
  for (full_path, entry_name, size) in all_files {
    // The per-file chunk limit is already validated above, before the cleanup.

    let current_archive_size = *crate::utils::locks::lock(&current_archive_size_ref);

    // Rollover to the next archive BEFORE adding the file so that no single
    // part exceeds the configured chunk size.
    if current_archive_size + size > max_size && current_group_entries > 0 {
      zip.finish().map_err(|e| e.to_string())?;

      let archive_name = format!("data{}.zip", part_number);
      emit_hashing_progress(app, &archive_name, total_size, processed_size);
      close_archive(out_dir, archive_name, current_group_entries, &mut manifest, &mut compressed_size)?;

      part_number += 1;
      let archive_path = out_dir.join(format!("data{}.zip", part_number));
      // Создаем новый чанк, обнуляя тот же счетчик
      zip = create_chunk(archive_path, Arc::clone(&shared_written))?;
      current_group_entries = 0;
    }

    let str_file_name = if cfg!(windows) {
      entry_name.to_string_lossy().replace('\\', "/")
    } else {
      entry_name.to_string_lossy().into_owned()
    };

    // Эмит прогресса ПЕРЕД началом сжатия файла
    let percentage = if total_size > 0 { (processed_size as f64 / total_size as f64) * 100.0 } else { 0.0 };
    let _ = app.emit(
      "packing-progress",
      CompressProgressPayload {
        status: 1,
        current_file: str_file_name.clone(),
        total_size,
        processed_size,
        percentage,
      },
    );

    // Open the file BEFORE creating the zip entry. If start_file runs first
    // and File::open fails, a zero-length entry is left in the archive (R2 fix).
    let f = match std::fs::File::open(&full_path) {
      Ok(f) => f,
      Err(e) => {
        log::warn!("Cannot open '{}', skipping: {}", full_path.display(), e);
        total_size = total_size.saturating_sub(size);
        skipped_files.push(str_file_name.clone());
        continue;
      }
    };

    let str_file_name = entry_name.to_string_lossy().replace('\\', "/");
    zip.start_file(&str_file_name, options).map_err(|e| e.to_string())?;

    let mut reader = std::io::BufReader::new(f);
    std::io::copy(&mut reader, &mut zip).map_err(|e| e.to_string())?;

    processed_size += size;
    current_group_entries += 1;
    packed_files_count += 1;
  }

  zip.finish().map_err(|e| e.to_string())?;

  let archive_name = format!("data{}.zip", part_number);
  emit_hashing_progress(app, &archive_name, total_size, processed_size);
  // An empty last part (rollover happened and every remaining file was skipped)
  // is discarded here instead of being hashed into the manifest (item 88).
  close_archive(out_dir, archive_name, current_group_entries, &mut manifest, &mut compressed_size)?;

  if manifest.files.is_empty() {
    return Err(format!(
      "{}: {} of {} source files could not be read",
      consts::PACK_ERR_NOTHING_PACKED,
      skipped_files.len(),
      all_files_count
    ));
  }

  manifest.total_files_count = packed_files_count;

  // Extra raw files (dev/test hook for future engine `.db` archives): copy
  // into the pack dir under a flat name and record with kind = Raw. Flat name
  // is required — download names must be simple file names (safe_download_join).
  for (src, normalized_target, flat_name) in raw_plan {
    if manifest.files.iter().any(|f| f.name == flat_name) {
      return Err(format!("extra_raw_files name collision with an existing pack file: {}", &flat_name));
    }

    let dest = out_dir.join(&flat_name);
    fs::copy(&src, &dest).map_err(|e| format!("copy raw file {:?}: {}", &src, e))?;

    let _ = app.emit(
      "packing-progress",
      CompressProgressPayload {
        status: 2,
        current_file: flat_name.clone(),
        total_size: 0,
        processed_size: 0,
        percentage: 0.,
      },
    );
    let sha = crate::utils::hash::sha256_file(&dest, None, None).map_err(|e| e.to_string())?;
    let size = dest.metadata().map_err(|e| e.to_string())?.len();

    log::info!("Added raw file {}: {} bytes, sha256 {}", &flat_name, size, &sha);
    compressed_size += size;
    total_size += size;
    manifest.total_files_count += 1;
    manifest.files.push(ReleaseManifestFile {
      name: flat_name,
      size,
      sha256: Some(sha),
      kind: ManifestFileKind::Raw,
      target: Some(normalized_target),
    });
  }

  manifest.compressed_size = compressed_size;
  manifest.total_size = total_size;

  // Patch metadata lands in the manifest as-is (full releases leave it empty).
  if let Some(pm) = &patch_meta {
    manifest.patch_name = Some(pm.patch_name.clone());
    manifest.base_patch = pm.base_patch.clone().filter(|s| !s.is_empty());
    manifest.base_release_tag = pm.base_release_tag.clone().filter(|s| !s.is_empty());
    manifest.deleted_files = pm.deleted_files.clone();
    manifest.updated_fields = pm.updated_fields.clone();
  }

  let manifest_path = Path::new(&targetPath).join(MANIFEST_NAME);
  let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
  // Atomic write: dump into a temp file next to the target, then rename over it.
  // A crash mid-write can no longer leave a truncated manifest.json.
  let tmp_path = manifest_path.with_extension(format!("json.{}.tmp", std::process::id()));
  fs::write(&tmp_path, &json).map_err(|e| e.to_string())?;
  fs::rename(&tmp_path, &manifest_path).map_err(|e| e.to_string())?;

  let _ = app.emit(
    "packing-progress",
    CompressProgressPayload {
      status: 1,
      current_file: "".to_owned(),
      total_size,
      processed_size,
      percentage: 100.,
    },
  );

  if !skipped_files.is_empty() {
    log::warn!(
      "Pack finished with {} skipped file(s): {}",
      skipped_files.len(),
      skipped_files.join(", ")
    );
  }

  Ok((manifest, skipped_files))
}

/// Unpack a single `.zip` archive into `outputDir`.
/// Multi-volume `.7z` is not supported — use Pack's `dataN.zip` archives.
#[tauri::command]
pub async fn extract_archive(
  service_unpack: tauri::State<'_, Arc<crate::service::unpack::ServiceUnpacker>>,
  versionName: String,
  archivePath: String,
  outputDir: String,
) -> Result<(), String> {
  let archive = PathBuf::from(&archivePath);
  let output = PathBuf::from(&outputDir);

  if !archive.is_file() {
    return Err(format!("Archive not found: {}", archivePath));
  }

  let name_lower = archive
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or("")
    .to_ascii_lowercase();
  if name_lower.contains(".7z") {
    return Err("7z archives are not supported. Use .zip (e.g. data1.zip from Pack).".to_string());
  }
  if !name_lower.ends_with(".zip") {
    return Err(format!("Unsupported archive type (expected .zip): {}", archivePath));
  }

  let file_label = archive
    .file_name()
    .and_then(|n| n.to_str())
    .unwrap_or("archive.zip")
    .to_string();

  let unpacker = service_unpack.inner().clone();
  tokio::task::spawn_blocking(move || {
    unpacker
      .extract_zip(&versionName, &file_label, &archive, &output)
      .map_err(|e| e.to_string())
  })
  .await
  .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
  use super::*;

  fn temp_pack_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gw_pack_{}_{}", tag, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp pack dir");
    dir
  }

  #[test]
  fn pack_archive_name_matches_only_data_parts() {
    assert!(is_pack_archive_name("data1.zip"));
    assert!(is_pack_archive_name("data42.zip"));
    assert!(!is_pack_archive_name("data.zip"));
    assert!(!is_pack_archive_name("dataX.zip"));
    assert!(!is_pack_archive_name("gamedata1.zip"));
    assert!(!is_pack_archive_name("data1.zip.tmp"));
  }

  /// Item 72: raw files of the previous run must not survive the cleanup.
  #[test]
  fn cleanup_removes_raw_files_listed_in_the_previous_manifest() {
    let dir = temp_pack_dir("cleanup72");

    let prev = ReleaseManifest {
      schema: 2,
      total_files_count: 3,
      total_size: 3,
      compressed_size: 3,
      files: vec![
        ReleaseManifestFile {
          name: "data1.zip".to_owned(),
          size: 1,
          sha256: Some("aa".to_owned()),
          kind: ManifestFileKind::Zip,
          target: None,
        },
        ReleaseManifestFile {
          name: "gamedata.db0".to_owned(),
          size: 1,
          sha256: Some("bb".to_owned()),
          kind: ManifestFileKind::Raw,
          target: Some("gamedata.db0".to_owned()),
        },
      ],
      exe_path: None,
      patch_name: None,
      base_patch: None,
      base_release_tag: None,
      deleted_files: vec![],
      updated_fields: vec![],
    };
    fs::write(dir.join(MANIFEST_NAME), serde_json::to_string(&prev).unwrap()).unwrap();
    fs::write(dir.join("data1.zip"), b"old").unwrap();
    fs::write(dir.join("data2.zip"), b"old").unwrap();
    fs::write(dir.join("gamedata.db0"), b"old raw").unwrap();
    fs::write(dir.join("readme.txt"), b"not ours").unwrap();

    cleanup_previous_pack(&dir);

    assert!(!dir.join(MANIFEST_NAME).exists(), "manifest must be removed");
    assert!(!dir.join("data1.zip").exists(), "old part must be removed");
    assert!(!dir.join("data2.zip").exists(), "old part must be removed");
    assert!(!dir.join("gamedata.db0").exists(), "raw file from the old manifest must be removed");
    assert!(dir.join("readme.txt").exists(), "unrelated files must be kept");

    let _ = fs::remove_dir_all(&dir);
  }

  /// Cleanup must survive a corrupted previous manifest without touching
  /// unrelated files.
  #[test]
  fn cleanup_survives_broken_previous_manifest() {
    let dir = temp_pack_dir("cleanup_broken");
    fs::write(dir.join(MANIFEST_NAME), b"{ not json").unwrap();
    fs::write(dir.join("data1.zip"), b"old").unwrap();
    fs::write(dir.join("keep.me"), b"keep").unwrap();

    cleanup_previous_pack(&dir);

    assert!(!dir.join(MANIFEST_NAME).exists());
    assert!(!dir.join("data1.zip").exists());
    assert!(dir.join("keep.me").exists());

    let _ = fs::remove_dir_all(&dir);
  }

  /// Item 88: a part that got no entries is deleted and never recorded.
  #[test]
  fn empty_archive_is_discarded_instead_of_being_recorded() {
    let dir = temp_pack_dir("close88");
    fs::write(dir.join("data3.zip"), b"PK\x05\x06").unwrap();

    let mut manifest = ReleaseManifest::default();
    let mut compressed = 0u64;
    close_archive(&dir, "data3.zip".to_owned(), 0, &mut manifest, &mut compressed).unwrap();

    assert!(manifest.files.is_empty(), "empty part must not reach the manifest");
    assert_eq!(compressed, 0, "empty part must not add compressed size");
    assert!(!dir.join("data3.zip").exists(), "empty part must be deleted from the pack dir");

    let _ = fs::remove_dir_all(&dir);
  }

  /// Counterpart of the test above: a non-empty part is hashed and recorded.
  #[test]
  fn non_empty_archive_is_hashed_and_recorded() {
    let dir = temp_pack_dir("close88_ok");
    fs::write(dir.join("data1.zip"), b"payload").unwrap();

    let mut manifest = ReleaseManifest::default();
    let mut compressed = 0u64;
    close_archive(&dir, "data1.zip".to_owned(), 2, &mut manifest, &mut compressed).unwrap();

    assert_eq!(manifest.files.len(), 1);
    assert_eq!(manifest.files[0].name, "data1.zip");
    assert_eq!(manifest.files[0].size, 7);
    assert!(manifest.files[0].sha256.is_some());
    assert_eq!(compressed, 7);
    assert!(dir.join("data1.zip").exists());

    let _ = fs::remove_dir_all(&dir);
  }
}
