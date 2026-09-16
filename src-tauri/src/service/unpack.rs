use std::fs;
use std::path::Path;
use std::sync::Arc;
use zip::ZipArchive;

use crate::consts::{UNPACK_EXTRACTED_LOG_LIMIT, UNPACK_SKIPPED_LOG_LIMIT};

pub type NetSpeedCallback = Box<dyn Fn(&str, &str, usize, usize) + Send + Sync>;

/// Files that were written to disk before an error interrupted the extraction.
/// The name list is capped (`UNPACK_EXTRACTED_LOG_LIMIT`) so a 50k-entry
/// archive cannot blow up memory, while `total` still reports the real count.
#[derive(Default)]
struct ExtractedFiles {
  total: usize,
  names: Vec<String>,
}

impl ExtractedFiles {
  fn push(&mut self, name: String) {
    self.total += 1;
    if self.names.len() < UNPACK_EXTRACTED_LOG_LIMIT {
      self.names.push(name);
    }
  }
}

pub struct ServiceUnpacker {
  callback: Arc<NetSpeedCallback>,
}

impl ServiceUnpacker {
  pub fn new<F>(callback: F) -> Self
  where
    F: Fn(&str, &str, usize, usize) + Send + Sync + 'static,
  {
    Self {
      callback: Arc::new(Box::new(callback)),
    }
  }

  /// Extract `file_path` into `extract_to`.
  ///
  /// On failure the files already written to disk are listed in the log: the
  /// extraction has no rollback, so that list is the only way to tell what a
  /// half-updated install contains.
  pub fn extract_zip(
    &self,
    release_name: &str,
    file_name: &str,
    file_path: &Path,
    extract_to: &Path,
  ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut extracted = ExtractedFiles::default();
    let result = self.extract_zip_inner(release_name, file_name, file_path, extract_to, &mut extracted);

    if let Err(ref e) = result {
      log::error!("Failed to extract {:?} into {:?}: {}", file_path, extract_to, e);
      log_extracted_files(extract_to, &extracted);
    }

    result
  }

  fn extract_zip_inner(
    &self,
    release_name: &str,
    file_name: &str,
    file_path: &Path,
    extract_to: &Path,
    extracted: &mut ExtractedFiles,
  ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let file = fs::File::open(file_path)?;
    let mut archive = ZipArchive::new(file)?;

    if !extract_to.exists() {
      fs::create_dir_all(extract_to)?;
    }

    let total_files = archive.len();
    let mut skipped: Vec<String> = Vec::new();

    // Empty archive — signal completion so the UI does not stay stuck at "unpacking".
    if total_files == 0 {
      (self.callback)(release_name, file_name, 1, 1);
      return Ok(());
    }

    // ~2 Hz throttle, same policy as the download progress in `ServiceFiles`:
    // the frontend rebuilds its whole progress state on every event, and a
    // dataN.zip with tens of thousands of entries fired one event per entry,
    // which froze the UI for the length of the extraction. The guaranteed
    // final 100% call still happens below.
    const PROGRESS_EMIT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
    let mut last_emit = std::time::Instant::now();

    for i in 0..total_files {
      // Первый вызов — сразу (иначе прогресс стоит на нуле до 500 мс),
      // дальше не чаще PROGRESS_EMIT_INTERVAL.
      if i == 0 || last_emit.elapsed() >= PROGRESS_EMIT_INTERVAL {
        (self.callback)(release_name, file_name, i, total_files);
        last_emit = std::time::Instant::now();
      }

      let mut file = archive.by_index(i)?;

      // Ключевое изменение: получаем путь, который безопасен для системы
      let outpath = match file.enclosed_name() {
        Some(path) => {
          // Validate each path component for Windows-illegal characters and
          // reserved device names (CON, NUL, COM1 …).
          if !is_safe_path(&path) {
            let name = file.name().to_string();
            log::warn!("Skipping archive entry with unsafe path component: {}", &name);
            skipped.push(name);
            continue;
          }
          extract_to.join(path)
        }
        None => {
          let name = file.name().to_string();
          log::warn!("Skipping suspicious archive entry: {}", &name);
          skipped.push(name);
          continue;
        }
      };

      // Используем встроенный метод is_dir() вместо проверки на '/'
      if file.is_dir() {
        fs::create_dir_all(&outpath)?;
      } else {
        // Убеждаемся, что родительская директория существует
        if let Some(p) = outpath.parent() {
          if !p.exists() {
            fs::create_dir_all(p)?;
          }
        }

        // Remove read-only attribute so File::create does not fail midway
        // through extraction (leaving a half-updated install).
        if outpath.exists() {
          let mut perms = fs::metadata(&outpath)?.permissions();
          if perms.readonly() {
            perms.set_readonly(false);
            fs::set_permissions(&outpath, perms)?;
          }
        }

        // Retry file creation up to 3 times on transient permission errors
        let mut outfile = None;
        let mut last_err = None;
        for attempt in 0..3u32 {
          match fs::File::create(&outpath) {
            Ok(f) => { outfile = Some(f); break; }
            Err(e) => {
              if attempt < 2 {
                log::warn!("File::create attempt {} failed for {:?}: {}", attempt + 1, outpath, e);
                std::thread::sleep(std::time::Duration::from_millis(100 * (attempt as u64 + 1)));
              }
              last_err = Some(e);
            }
          }
        }
        let mut outfile = outfile.ok_or_else(|| {
          let e = last_err.unwrap();
          format!("Failed to create {:?}: {}", outpath, e)
        })?;

        // Name the file in the error: a bare `?` here reported only "disk full"
        // or "access denied" with no way to tell which entry died.
        let entry_name = file.name().to_string();
        std::io::copy(&mut file, &mut outfile).map_err(|e| {
          format!("Failed to write {:?} (archive entry '{}'): {}", outpath, entry_name, e)
        })?;

        extracted.push(outpath.to_string_lossy().into_owned());
      }

      // Установка прав доступа для Unix
      #[cfg(unix)]
      {
        use std::os::unix::fs::PermissionsExt;
        if let Some(mode) = file.unix_mode() {
          fs::set_permissions(&outpath, fs::Permissions::from_mode(mode))?;
        }
      }
    }

    // Skipping is a warning, not a failure. Failing here used to mark the whole
    // install as unsuccessful *after* the archive had already been written to
    // disk (and there is no rollback), so a single mod folder whose name ends
    // with a space made every install of that version fail forever.
    if !skipped.is_empty() {
      log::warn!(
        "Skipped {} archive entries with unsafe names while extracting {:?}: {}{}",
        skipped.len(),
        file_path,
        skipped
          .iter()
          .take(UNPACK_SKIPPED_LOG_LIMIT)
          .cloned()
          .collect::<Vec<_>>()
          .join(", "),
        if skipped.len() > UNPACK_SKIPPED_LOG_LIMIT { ", …" } else { "" }
      );
    }

    // Финальный вызов callback
    (self.callback)(release_name, file_name, total_files, total_files);
    log::info!(
      "Successfully extracted {:?} to {:?} ({} files written, {} skipped)",
      file_path, extract_to, extracted.total, skipped.len()
    );

    Ok(())
  }
}

/// Dump what already landed on disk so a broken install can be cleaned up by
/// hand. The destination directory is logged too, since the name list is capped.
fn log_extracted_files(extract_to: &Path, extracted: &ExtractedFiles) {
  if extracted.total == 0 {
    log::warn!("No files had been extracted into {:?} before the failure", extract_to);
    return;
  }

  log::warn!(
    "{} file(s) were already extracted into {:?} and are NOT rolled back; first {}: {}",
    extracted.total,
    extract_to,
    extracted.names.len(),
    extracted.names.join(", ")
  );
}

/// Windows reserved device names that must not appear as file/directory names.
#[cfg(windows)]
const RESERVED_NAMES: &[&str] = &[
  "CON", "PRN", "AUX", "NUL",
  "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
  "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Check that every component of a relative path is safe to create on disk:
/// no forbidden characters (`< > : " | ? *`), no trailing dots/spaces, and no
/// Windows reserved device names.
///
/// All of these are Windows-only restrictions, so on other platforms every
/// `enclosed_name()` is accepted (path traversal is already blocked by `zip`).
#[cfg(windows)]
fn is_safe_path(path: &std::path::Path) -> bool {
  for component in path.components() {
    let name = match component {
      std::path::Component::Normal(n) => n.to_string_lossy(),
      _ => continue,
    };

    // Forbidden characters on Windows
    if name.contains(|c: char| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*')) {
      return false;
    }

    // Trailing dots or spaces are silently stripped by Windows, causing confusion
    if name.ends_with('.') || name.ends_with(' ') {
      return false;
    }

    // Reserved device names (case-insensitive, with or without extension)
    let stem = name.split('.').next().unwrap_or(&name);
    if RESERVED_NAMES.contains(&stem.to_uppercase().as_str()) {
      return false;
    }
  }
  true
}

#[cfg(not(windows))]
fn is_safe_path(_path: &std::path::Path) -> bool {
  true
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::PathBuf;

  #[test]
  fn extracted_files_caps_the_name_list_but_not_the_counter() {
    let mut extracted = ExtractedFiles::default();
    for i in 0..(UNPACK_EXTRACTED_LOG_LIMIT + 25) {
      extracted.push(format!("file{}.txt", i));
    }

    assert_eq!(extracted.total, UNPACK_EXTRACTED_LOG_LIMIT + 25);
    assert_eq!(extracted.names.len(), UNPACK_EXTRACTED_LOG_LIMIT);
    assert_eq!(extracted.names[0], "file0.txt");
  }

  #[cfg(windows)]
  #[test]
  fn is_safe_path_rejects_windows_only_hazards() {
    assert!(!is_safe_path(&PathBuf::from("gamedata/mod ")));
    assert!(!is_safe_path(&PathBuf::from("gamedata/mod./file.ltx")));
    assert!(!is_safe_path(&PathBuf::from("gamedata/nul.txt")));
    assert!(!is_safe_path(&PathBuf::from("gamedata/a?b.ltx")));
  }

  #[cfg(windows)]
  #[test]
  fn is_safe_path_accepts_normal_entries() {
    assert!(is_safe_path(&PathBuf::from("gamedata/configs/alife.ltx")));
    assert!(is_safe_path(&PathBuf::from("bin/xrEngine.exe")));
  }

  #[cfg(not(windows))]
  #[test]
  fn is_safe_path_is_permissive_off_windows() {
    assert!(is_safe_path(&PathBuf::from("gamedata/mod ")));
    assert!(is_safe_path(&PathBuf::from("gamedata/nul.txt")));
  }
}
