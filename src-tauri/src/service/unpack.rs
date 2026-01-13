use std::fs;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
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
    // Открываем архив
    let file = fs::File::open(file_path)?;
    let mut archive = ZipArchive::new(file)?;

    // Создаем целевую директорию, если её нет
    if !extract_to.exists() {
      fs::create_dir_all(extract_to)?;
    }

    for i in 0..archive.len() {
      (self.callback)(release_name, &file_name, i, archive.len());

      let mut file = archive.by_index(i)?;

      // Получаем внутренний путь файла в архиве и очищаем его от потенциально опасных ".."
      let outpath = match file.enclosed_name() {
        Some(path) => extract_to.join(path),
        None => continue,
      };

      if file.name().ends_with('/') {
        // Если это директория — создаем её
        fs::create_dir_all(&outpath)?;
      } else {
        // Если это файл — создаем родительские папки, если их еще нет
        if let Some(p) = outpath.parent() {
          if !p.exists() {
            fs::create_dir_all(p)?;
          }
        }
        // Распаковываем файл
        let mut outfile = fs::File::create(&outpath)?;
        std::io::copy(&mut file, &mut outfile)?;
      }

      // В Unix-системах сохраняем права доступа (опционально)
      #[cfg(unix)]
      {
        use std::os::unix::fs::PermissionsExt;
        if let Some(mode) = file.unix_mode() {
          fs::set_permissions(&outpath, fs::Permissions::from_mode(mode))?;
        }
      }
    }

    (self.callback)(release_name, &file_name, archive.len(), archive.len());

    log::info!("Successfully extracted {:?} to {:?}", file_path, extract_to);
    Ok(())
  }
}
