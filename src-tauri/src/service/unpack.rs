use std::fs;
use std::path::Path;
use std::sync::Arc;
use zip::ZipArchive;

pub type NetSpeedCallback = Box<dyn Fn(&str, &str, usize, usize) + Send + Sync>;

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

  pub fn extract_zip(
    &self,
    release_name: &str,
    file_name: &str,
    file_path: &Path,
    extract_to: &Path,
  ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let file = fs::File::open(file_path)?;
    let mut archive = ZipArchive::new(file)?;

    if !extract_to.exists() {
      fs::create_dir_all(extract_to)?;
    }

    let total_files = archive.len();

    for i in 0..total_files {
      // Вызываем callback перед обработкой файла
      (self.callback)(release_name, file_name, i, total_files);

      let mut file = archive.by_index(i)?;

      // Ключевое изменение: получаем путь, который безопасен для системы
      let outpath = match file.enclosed_name() {
        Some(path) => extract_to.join(path),
        None => {
          log::warn!("Пропущен подозрительный файл в архиве: {}", file.name());
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

        // Распаковка файла
        let mut outfile = fs::File::create(&outpath)?;
        std::io::copy(&mut file, &mut outfile)?;
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

    // Финальный вызов callback
    (self.callback)(release_name, file_name, total_files, total_files);
    log::info!("Successfully extracted {:?} to {:?}", file_path, extract_to);

    Ok(())
  }
}
