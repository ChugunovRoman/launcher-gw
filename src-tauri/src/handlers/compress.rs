use crate::consts::MANIFEST_NAME;
use crate::handlers::dto::{CompressProgressPayload, ReleaseManifest, ReleaseManifestFile};
use crate::utils::CountingWriter::CountingWriter;
use anyhow::Result;
use globset::{Glob, GlobSetBuilder};
use std::fs::{self, File, Metadata};
use std::io::BufWriter;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use tauri::Emitter;
use walkdir::WalkDir;
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

#[tauri::command]
pub async fn create_split_archives(
  app: tauri::AppHandle,
  sourceDir: String,
  targetPath: String,
  chunkSize: u64,
  excludePatterns: Vec<String>,
) -> Result<(), String> {
  let src_dir = Path::new(&sourceDir);
  let out_dir = Path::new(&targetPath);
  let max_size = chunkSize * 1024 * 1024;

  let mut builder = GlobSetBuilder::new();
  for pattern in excludePatterns {
    builder.add(Glob::new(&pattern).map_err(|e| e.to_string())?);
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

  println!("[DEBUG] Start copmress. Searching files in: {}", &sourceDir);
  for entry in WalkDir::new(src_dir).into_iter().filter_map(|e| e.ok()) {
    let full_path = entry.path();
    let relative_path = full_path.strip_prefix(src_dir).map_err(|e| e.to_string())?;

    if setter.is_match(relative_path) {
      continue;
    }

    if entry.file_type().is_file() {
      let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
      total_size += size;
      all_files.push((full_path.to_path_buf(), relative_path.to_path_buf(), size));
    }
  }
  println!(
    "[DEBUG] Search files completed ! all_files: {} total_size: {}",
    &all_files.len(),
    total_size
  );

  let mut manifest = ReleaseManifest {
    total_files_count: all_files.len() as u32,
    total_size,
    compressed_size: 0,
    files: vec![],
  };

  // 2. Процесс упаковки
  let mut processed_size = 0;
  let mut part_number = 1;
  let mut current_group_size = 0;
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

  for (full_path, entry_name, size) in all_files {
    let current_archive_size = *current_archive_size_ref.lock().unwrap();

    if current_archive_size > max_size && current_group_size > 0 {
      zip.finish().map_err(|e| e.to_string())?;

      let archive_name = format!("data{}.zip", part_number);
      let file_path = out_dir.join(&archive_name);
      let meta = file_path.metadata().map_err(|e| e.to_string())?;
      let size = meta.file_size();
      compressed_size += size;

      manifest.files.push(ReleaseManifestFile { name: archive_name, size });

      part_number += 1;
      let archive_path = out_dir.join(format!("data{}.zip", part_number));
      // Создаем новый чанк, обнуляя тот же счетчик
      zip = create_chunk(archive_path, Arc::clone(&shared_written))?;
      current_group_size = 0;
    }

    let str_file_name = if cfg!(windows) {
      entry_name.to_string_lossy().replace('\\', "/")
    } else {
      entry_name.to_string_lossy().into_owned()
    };

    // Эмит прогресса ПЕРЕД началом сжатия файла
    let percentage = (processed_size as f64 / total_size as f64) * 100.0;
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

    let str_file_name = entry_name.to_string_lossy().replace('\\', "/");
    zip.start_file(&str_file_name, options).map_err(|e| e.to_string())?;

    let f = std::fs::File::open(&full_path).map_err(|e| e.to_string())?;
    let mut reader = std::io::BufReader::new(f);
    std::io::copy(&mut reader, &mut zip).map_err(|e| e.to_string())?;

    processed_size += size;
    current_group_size += size;
  }

  let file_path = out_dir.join(format!("data{}.zip", part_number));
  let meta = file_path.metadata().map_err(|e| e.to_string())?;
  let size = meta.file_size();
  compressed_size += size;

  manifest.files.push(ReleaseManifestFile {
    name: format!("data{}.zip", part_number),
    size: *current_archive_size_ref.lock().unwrap(),
  });

  manifest.compressed_size = compressed_size;

  let manifest_path = Path::new(&targetPath).join(MANIFEST_NAME).to_string_lossy().into_owned();
  let json = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
  fs::write(&manifest_path, json).map_err(|e| e.to_string())?;

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

  zip.finish().map_err(|e| e.to_string())?;
  Ok(())
}
