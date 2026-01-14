use crate::{
  configs::AppConfig::AppConfig,
  consts::PULL_FILES_SIZE,
  handlers::{
    dto::{DownlaodFileStat, DownloadProgress, DownloadStatus, UnzipTask},
    start_download_version::CancelMap,
  },
  service::{files::ServiceFiles, main::Service, unpack::ServiceUnpacker},
};
use std::{cmp::Reverse, fs, path::Path, sync::Arc, time::Duration};
use std::{
  path::PathBuf,
  sync::atomic::{AtomicU16, Ordering},
};
use tauri::Emitter;
use tokio::sync::{Mutex, broadcast, mpsc};

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
  let (cancel_tx, _) = broadcast::channel::<()>(1);
  {
    channel_map.lock().unwrap().insert(versionName.clone(), cancel_tx.clone());
  }
  scopeguard::defer! {
    channel_map.lock().unwrap().remove(&versionName);
  };

  // 2. Сбор статистики и подготовка данных
  let mut file_sizes: Vec<DownlaodFileStat> = vec![];
  let (version, mut files_to_download, files_to_unpack) = {
    let mut cfg_guard = app_config.lock().await;
    let mut to_download = Vec::new();
    let mut to_unpack = Vec::new();
    let version_data = {
      let version_data = cfg_guard
        .progress_download
        .get_mut(&versionName)
        .ok_or_else(|| "Version not found".to_string())?;

      let mut files_dwn_cnt: u16 = 0;

      for (_, file_progress) in version_data.files.iter_mut() {
        let file_path = Path::new(&version_data.download_path).join(&file_progress.name);
        let file_part_path = Path::new(&version_data.download_path).join(format!("{}.part", &file_progress.name));

        let current_size = if file_part_path.exists() {
          tokio::fs::read_to_string(&file_part_path)
            .await
            .ok()
            .and_then(|s| s.trim().parse::<u64>().ok())
            .unwrap_or(0)
        } else if file_path.exists() && file_progress.is_downloaded {
          file_progress.total_size
        } else if !file_path.exists() && file_progress.is_unpacked {
          file_progress.total_size
        } else {
          0
        };

        file_progress.size = current_size;

        if current_size >= file_progress.total_size && file_progress.total_size > 0 {
          file_progress.is_downloaded = true;
          files_dwn_cnt += 1;
        } else {
          file_progress.is_downloaded = false;
          to_download.push(file_progress.clone());
        }

        if file_progress.is_downloaded && !file_progress.is_unpacked {
          to_unpack.push(file_progress.clone());
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

    (version_data.clone(), to_download, to_unpack)
  };

  // Сортировка для UI (по номеру чанка в расширении)
  file_sizes.sort_by_key(|file| Reverse(file.size));
  files_to_download.sort_by_key(|file| Reverse(file.size));

  let _ = app.emit("download-version-files", (&versionName, &file_sizes));
  let progress = (version.downloaded_files_cnt as f32 / version.total_file_count as f32) * 100.0;
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

  // 3. Создание очереди задач
  let total_file_count = version.total_file_count;
  let downloaded_cnt = Arc::new(AtomicU16::new(version.downloaded_files_cnt));
  let (tx_queue, rx_queue) = mpsc::channel(total_file_count as usize + 100);

  for file in files_to_download {
    log::debug!("tx_queue.send, file: {:?}", &file);
    let _ = tx_queue.send(file).await;
  }

  let (tx_unzip, mut rx_unzip) = mpsc::channel::<UnzipTask>(total_file_count as usize);

  // Отдельный поток-менеджер распаковки
  let app_unzip = app.clone();
  let version_name_unzip = versionName.clone();
  let service_unpack_arc = service_unpack.inner().clone();
  let app_config_arc = app_config.inner().clone();
  let unzip_manager_handle = tokio::spawn(async move {
    while let Some(data) = rx_unzip.recv().await {
      log::debug!("Worker got msg to unpack file, data: {:?}", &data);

      let app_inner = app_unzip.clone();
      let v_name = version_name_unzip.clone();
      let service_unpack_for_thread = service_unpack_arc.clone();
      let app_config_arc_for_thread = app_config_arc.clone();

      // Используем spawn_blocking, так как распаковка — это CPU-intensive задача
      tokio::task::spawn_blocking(move || {
        let _ = service_unpack_for_thread.extract_zip(&v_name, &data.file_name, &data.archive_path, &data.destination_path);
        let _ = app_inner.emit("file-unzipped", (&v_name, data.archive_path.to_str()));
        // Для работы с async Mutex внутри синхронного spawn_blocking
        // используем блокирующий вызов через текущий runtime
        let mut config_guard = tokio::runtime::Handle::current().block_on(app_config_arc_for_thread.lock());

        if let Some(ver) = config_guard.progress_download.get_mut(&v_name) {
          if let Some(file_progress) = ver.files.get_mut(&data.file_name) {
            file_progress.is_unpacked = true;
          }
        }
        let _ = config_guard.save();
        fs::remove_file(&data.archive_path).ok();
      })
      .await
      .ok();

      if data.is_latest {
        break;
      }
    }
    log::info!("Unzip queue finished");
  });

  let rx_queue_arc = Arc::new(Mutex::new(rx_queue));
  let cancel_tx_arc = Arc::new(cancel_tx);
  let tx_unzip_arc = Arc::new(tx_unzip);

  for file in files_to_unpack {
    let download_dir_c = Path::new(&version.download_path).to_path_buf();
    let file_path = download_dir_c.join(&file.name);
    let _ = tx_unzip_arc
      .send(UnzipTask {
        file_name: file.name.clone(),
        archive_path: file_path,
        destination_path: PathBuf::from(&version.installed_path),
        is_latest: downloaded_cnt.fetch_add(1, Ordering::SeqCst) >= total_file_count,
      })
      .await;
  }

  let api_client = service.lock().await.api_client.clone();
  let mut join_handles = Vec::new();

  // 4. Запуск воркеров
  for _ in 0..PULL_FILES_SIZE {
    let app_c = app.clone();
    let app_config_c = app_config.inner().clone();
    let service_files_c = service_files.inner().clone();
    let api_client_c = api_client.clone();
    let version_name_c = versionName.clone();
    let version_install_path_c = version.installed_path.clone();
    let download_dir_c = Path::new(&version.download_path).to_path_buf();
    let downloaded_cnt_c = downloaded_cnt.clone();

    let tx_unzip_c = tx_unzip_arc.clone();
    let rx_queue_c = rx_queue_arc.clone();
    let tx_queue_c = tx_queue.clone();
    let cancel_tx_arc_c = cancel_tx_arc.clone();
    let mut stop_rx = cancel_tx_arc.subscribe();

    let handle = tokio::spawn(async move {
      loop {
        let file_task = {
          let mut rx_lock = rx_queue_c.lock().await;
          tokio::select! {
              _ = stop_rx.recv() => break,
              task = rx_lock.recv() => match task {
                  Some(t) => t,
                  None => break,
              }
          }
        };

        let file_path = download_dir_c.join(&file_task.name);
        let part_path = format!("{}.part", file_path.to_str().unwrap_or(""));

        // Актуальный seek перед каждой попыткой
        let seek_pos = tokio::fs::read_to_string(&part_path)
          .await
          .ok()
          .and_then(|s| s.trim().parse::<u64>().ok());

        let mut local_cancel = cancel_tx_arc_c.subscribe();
        let res = service_files_c
          .download_blob_to_file(
            &api_client_c,
            &version_name_c,
            &file_task.download_link,
            &file_task.total_size,
            &file_path,
            &seek_pos,
            local_cancel,
          )
          .await;

        match res {
          Ok(_) => {
            let current = downloaded_cnt_c.fetch_add(1, Ordering::SeqCst) + 1;

            let _ = tx_unzip_c
              .send(UnzipTask {
                file_name: file_task.name.clone(),
                archive_path: file_path.clone(),
                destination_path: PathBuf::from(&version_install_path_c),
                is_latest: current >= total_file_count,
              })
              .await;

            // Обновляем конфиг
            {
              let mut config_guard = app_config_c.lock().await;
              if let Some(ver) = config_guard.progress_download.get_mut(&version_name_c) {
                if let Some(fp) = ver.files.get_mut(&file_task.id) {
                  fp.is_downloaded = true;
                }
                ver.downloaded_files_cnt = current;
              }
              let _ = config_guard.save();
            }

            // Эмит прогресса
            let progress = (current as f32 / total_file_count as f32) * 100.0;
            let _ = app_c.emit(
              "download-version",
              DownloadProgress {
                version_name: version_name_c.clone(),
                status: DownloadStatus::DownloadFiles,
                file: file_task.name,
                progress,
                downloaded_files_cnt: current,
                total_file_count,
              },
            );

            if current >= total_file_count {
              let _ = cancel_tx_arc_c.send(());
              break;
            }
          }
          Err(e) => {
            log::error!("Retry required for {}: {}", file_task.name, e);
            tokio::time::sleep(Duration::from_secs(2)).await;
            let _ = tx_queue_c.send(file_task).await;
          }
        }
      }
    });
    join_handles.push(handle);
  }

  // 5. Ожидание завершения
  drop(tx_queue); // Позволяет rx_lock.recv() вернуть None, когда воркеры закончат ретраи

  for h in join_handles {
    let _ = h.await;
  }

  // ВАЖНО: Закрываем передатчик очереди распаковки.
  // После этого rx_unzip.recv() вернет None, когда обработает ВСЕ задачи в очереди.
  drop(tx_unzip_arc);

  // Ждем, пока менеджер распаковки закончит последний файл
  let _ = unzip_manager_handle.await;

  let _ = app.emit("download-unpack-version", &versionName);

  Ok(())
}
