use crate::consts::MANIFEST_NAME;
use crate::handlers::dto::ReleaseManifest;
use crate::utils::errors::log_full_error;
use crate::utils::parse_strings::*;
use crate::utils::{self, resources};
use anyhow::Result;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tauri::Emitter;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::sync::Mutex;

#[tauri::command]
pub async fn create_archive(
  app_handle: tauri::AppHandle,
  sourceDir: String,
  targetPath: String,
  excludePatterns: Vec<String>,
) -> Result<String, String> {
  let sevenz = resources::get_sevenz_path(&app_handle).map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  let archive_path = Path::new(&targetPath).join("game");
  let manifest_path = Path::new(&targetPath).join(MANIFEST_NAME).to_string_lossy().into_owned();

  log::info!("create_archive, clear dir: {:?}", &targetPath);

  utils::paths::clear_dir(targetPath).expect("Не удалось очистить папку");

  log::info!("create_archive, sevenz: {:?}", &sevenz);

  let mut args = vec![
    "a".to_string(),
    "-t7z".to_string(),
    "-m0=lzma2".to_string(),
    "-mx=9".to_string(),
    "-mfb=64".to_string(),
    "-md=1g".to_string(), // 1 ГБ словарь — для максимального сжатия
    "-ms=on".to_string(), // solid archive
    "-v50m".to_string(),  // тома по 50 МБ
    "-bsp1".to_string(),  // вывод прогресса в stdout
    "-bb3".to_string(),
    archive_path.to_string_lossy().into(),
    sourceDir,
  ];

  // Добавляем исключения: -x!pattern
  for pat in excludePatterns {
    args.push(format!("-xr!{}", pat));
  }

  log::info!("create_archive, start");
  log::info!("create_archive, command: {:?} {:?}", &sevenz, &args);

  let mut command = TokioCommand::new(sevenz);

  #[cfg(target_os = "windows")]
  {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
  }

  let mut child = command
    .args(args)
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .map_err(|e| e.to_string())?;

  let stderr = child.stdout.take().unwrap();
  let reader = BufReader::new(stderr);
  let mut lines = reader.lines();

  let manifest = Arc::new(Mutex::new(ReleaseManifest {
    total_files_count: 0,
    total_size: 0,
    compressed_size: 0,
  }));

  // Асинхронно читаем stderr
  let manifest_clone = Arc::clone(&manifest);
  let reader_handle = tokio::spawn(async move {
    while let Ok(Some(line)) = lines.next_line().await {
      log::info!("Print line: {}", &line);
      if let Ok(data) = extract_total(&line) {
        let mut m = manifest_clone.lock().await;
        log::info!("extract_total: {:?}", &data);
        m.total_files_count = data.0.clone();
        m.total_size = data.1.clone();
      }
      if let Ok(size) = extract_output(&line) {
        let mut m = manifest_clone.lock().await;
        log::info!("extract_output: {:?}", &size);
        m.compressed_size = size.clone();
      }
      if let Some(percent) = parse_progress(&line) {
        log::info!("parse_progress: {:?}", &percent);
        let _ = app_handle.emit("pack_archive_progress", percent);
      }
    }
  });

  // Ждём завершения
  let status = child.wait().await.map_err(|e| e.to_string())?;
  let _ = reader_handle.await.map_err(|e| e.to_string())?;

  let final_manifest = manifest.lock().await;
  log::info!("Created manifest: {:?}", &*final_manifest);
  log::info!("manifest path: {:?}", &manifest_path);

  let json = serde_json::to_string_pretty(&*final_manifest).map_err(|e| e.to_string())?;
  fs::write(&manifest_path, json).map_err(|e| e.to_string())?;

  if !status.success() {
    return Err("7zz failed".to_string());
  }

  Ok("Archive created successfully".to_string())
}

#[tauri::command]
pub async fn extract_archive(
  app_handle: tauri::AppHandle,
  versionName: String,
  archivePath: String, // путь к первому тому (например, "backup.7z" или "backup.7z.001")
  outputDir: String,   // куда распаковывать
) -> Result<String, String> {
  let sevenz = resources::get_sevenz_path(&app_handle).map_err(|e| e.to_string())?;

  // Убедимся, что выходная директория существует
  std::fs::create_dir_all(&outputDir).map_err(|e| format!("Failed to create output directory: {}", e))?;

  log::info!("extract_archive, clear dir: {:?}", &outputDir);

  // utils::paths::clear_dir(&outputDir).expect("Не удалось очистить папку");

  // Проверим, что архив существует
  if !Path::new(&archivePath).exists() {
    return Err(format!("Archive not found: {}", archivePath));
  }

  let args = vec![
    "x".to_string(), // извлечь с сохранением путей
    archivePath,
    format!("-o{}", outputDir), // обязательно без пробела после -o
    "-y".to_string(),           // автоматически подтверждать перезапись
    "-bsp1".to_string(),
    "-bb3".to_string(),
  ];

  log::info!("extract_archive: running {:?} with args: {:?}", sevenz, args);

  let mut command = TokioCommand::new(sevenz);

  #[cfg(target_os = "windows")]
  {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
  }

  let mut child = command
    .args(args)
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .map_err(|e| e.to_string())?;

  let stderr = child.stdout.take().unwrap();
  let reader = BufReader::new(stderr);
  let mut lines = reader.lines();

  // Асинхронно читаем stderr
  tokio::spawn(async move {
    while let Ok(Some(line)) = lines.next_line().await {
      log::info!("Print line: {}", &line);
      if let Some(percent) = parse_progress(&line) {
        let _ = app_handle.emit("unpack_archive_progress", (&versionName, percent));
      }
    }
  });

  // Ждём завершения
  let status = child.wait().await.map_err(|e| e.to_string())?;

  if !status.success() {
    return Err("7zz failed".to_string());
  }

  log::info!("extract_archive: completed successfully");
  Ok("Archive extracted successfully".to_string())
}

fn parse_progress(line: &str) -> Option<u8> {
  // Пропускаем начальные пробелы
  let trimmed = line.trim_start();

  // Находим позицию символа '%'
  if let Some(percent_pos) = trimmed.find('%') {
    // Берём подстроку до '%'
    let percent_str = &trimmed[..percent_pos];

    // Убеждаемся, что это только цифры (возможно, с пробелами до — но trim_start уже сделан)
    if let Ok(pct) = percent_str.trim().parse::<u8>() {
      if pct <= 100 {
        return Some(pct);
      }
    }
  }
  None
}
