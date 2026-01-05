use crate::{
  configs::AppConfig::{AppConfig, FileProgress},
  handlers::{
    dto::{DownlaodFileStat, DownloadProgress, DownloadStatus},
    start_download_version::CancelMap,
  },
  service::{files::ServiceFiles, main::Service},
};
use std::sync::atomic::{AtomicU16, Ordering};
use std::{path::Path, sync::Arc};
use tauri::Emitter;
use tokio::sync::Semaphore;
use tokio::sync::{Mutex, broadcast};

#[tauri::command]
pub async fn continue_download_version(
  app: tauri::AppHandle,
  channel_map: tauri::State<'_, CancelMap>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  service_files: tauri::State<'_, Arc<ServiceFiles>>,
  versionName: String,
) -> Result<(), String> {
  log::info!("Start continue_download_version, selected_version: {:?}", &versionName);

  let (tx, mut rx) = broadcast::channel::<()>(1);
  {
    channel_map.lock().unwrap().insert(versionName.clone(), tx.clone());
  }
  // Удаляем запись после завершения (успешного или нет)
  scopeguard::defer! {
    channel_map.lock().unwrap().remove(&versionName);
  };

  let mut file_sizes: Vec<DownlaodFileStat> = vec![];
  let version = {
    let mut cfg_guard = app_config.lock().await;
    {
      let version_data = cfg_guard
        .progress_download
        .get_mut(&versionName)
        .ok_or_else(|| "Version not found".to_string())?;

      let mut files_dwn_cnt: u16 = 0;
      for file in version_data.files.iter_mut() {
        let file_path = Path::new(&version_data.download_path).join(&file.1.path);
        let file_part_path = Path::new(&version_data.download_path).join(format!("{}.part", &file.1.path));

        if file_path.exists() {
          let size = match tokio::fs::read_to_string(file_part_path).await {
            Ok(content) => content.trim().parse::<u64>().unwrap_or(0),
            Err(_) => file.1.total_size,
          };

          file_sizes.push(DownlaodFileStat {
            name: file.1.path.clone(),
            size: Some(size),
          });
          file.1.is_downloaded = if file.1.total_size == size {
            files_dwn_cnt += 1;
            true
          } else {
            false
          };
          file.1.size = size;
        } else {
          file_sizes.push(DownlaodFileStat {
            name: file.1.path.clone(),
            size: None,
          });
          file.1.is_downloaded = false;
        }
      }

      version_data.downloaded_files_cnt = files_dwn_cnt;
    }
    cfg_guard.save().map_err(|e| e.to_string())?;
    cfg_guard.progress_download.get(&versionName).cloned().unwrap()
  };

  file_sizes.sort_by_key(|file| file.name.split('.').last().and_then(|ext| ext.parse::<u32>().ok()).unwrap_or(0));

  let _ = app.emit("download-version-files", (&versionName, &file_sizes));

  // 3. Фильтруем файлы, которые нужно скачать
  let mut files_to_download: Vec<FileProgress> = version.files.values().filter(|f| !f.is_downloaded).cloned().collect();
  files_to_download.sort_by_key(|f| {
    let is_partial = f.size != 0 && f.size < f.total_size;
    !is_partial
  });

  let total_file_count = version.total_file_count;
  // Начальное количество — это общее минус те, что осталось скачать
  let initial_downloaded_count = version.downloaded_files_cnt;
  let downloaded_cnt = Arc::new(AtomicU16::new(initial_downloaded_count));

  let semaphore = Arc::new(Semaphore::new(6));
  let mut join_handles = Vec::new();

  // Эмит начального состояния
  let _ = app.emit(
    "download-version",
    DownloadProgress {
      version_name: version.name.clone(),
      status: DownloadStatus::Init,
      file: "".to_owned(),
      progress: (initial_downloaded_count as f32 / total_file_count as f32) * 100.0,
      downloaded_files_cnt: initial_downloaded_count,
      total_file_count,
    },
  );

  let download_dir = Path::new(&version.download_path).to_path_buf();

  // 4. Основной цикл раздачи задач
  for file in files_to_download {
    // Проверка отмены перед стартом следующей задачи
    if !rx.is_empty() || rx.try_recv().is_ok() {
      log::info!("Continue task '{}' was cancelled", &versionName);
      let _ = app.emit("cancel-download-version", &versionName);
      break;
    }

    // Ждем свободный слот в пуле (макс 6)
    let permit = semaphore.clone().acquire_owned().await.map_err(|e| e.to_string())?;

    // Логика по условию: ждем 1 секунду перед добавлением новой загрузки в пул
    // tokio::time::sleep(Duration::from_secs(1)).await;

    // Клонируем данные для перемещения в поток
    let app_c = app.clone();
    let app_config_c = app_config.inner().clone();
    let service_files_c = service_files.inner().clone();
    let service_c = service.inner().clone();
    let version_name_c = versionName.clone();
    let file_c = file.clone();
    let download_dir_c = download_dir.clone();
    let downloaded_cnt_c = downloaded_cnt.clone();
    let mut local_rx = tx.subscribe();
    let local_rx_2 = tx.subscribe();
    let seek = match file_sizes.iter().find(|f| f.name == file.name).cloned() {
      Some(data) => data.size,
      None => None,
    };

    let handle = tokio::spawn(async move {
      let _permit = permit; // Удерживаем слот до конца загрузки файла

      let file_path = download_dir_c.join(&file_c.path);
      let api_client = {
        let service_guard = service_c.lock().await;
        service_guard.api_client.clone()
      };

      log::info!("Starting parallel download: {:?} seek: {:?}", file_c.name, &seek);

      tokio::select! {
          _ = local_rx.recv() => {
              log::info!("File download cancelled: {}", file_c.name);
              let _ = app_c.emit("cancel-download-version", &version_name_c);
          }
          res = service_files_c.download_blob_to_file(
              &api_client,
              &version_name_c,
              &file_c.project_id,
              &file_c.id,
              &file_path,
              &seek,
              local_rx_2
          ) => {
              match res {
                  Ok(_) => {
                      // Успешная загрузка: обновляем счетчик и конфиг
                      let current = downloaded_cnt_c.fetch_add(1, Ordering::SeqCst) + 1;

                      {
                          let mut config_guard = app_config_c.lock().await;
                          if let Some(ver) = config_guard.progress_download.get_mut(&version_name_c) {
                              if let Some(fp) = ver.files.get_mut(&file_c.id) {
                                  fp.is_downloaded = true;
                              }
                              ver.downloaded_files_cnt = current;
                          }
                          let _ = config_guard.save();
                      }

                      let progress = (current as f32 / total_file_count as f32) * 100.0;
                      let _ = app_c.emit("download-version", DownloadProgress {
                          version_name: version_name_c,
                          status: DownloadStatus::DownloadFiles,
                          file: file_c.name,
                          progress,
                          downloaded_files_cnt: current,
                          total_file_count,
                      });
                  },
                  Err(e) => {
                      log::error!("Failed to download {}: {}", file_c.name, e);
                  }
              }
          }
      }
    });

    join_handles.push(handle);
  }

  // 5. Ожидание завершения всех запущенных потоков
  for h in join_handles {
    let _ = h.await;
  }

  // Финальное событие, если не было отмены
  if rx.is_empty() {
    let _ = app.emit("download-unpack-version", &versionName);
  }

  Ok(())
}
