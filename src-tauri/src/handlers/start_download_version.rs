use crate::{
  configs::AppConfig::{AppConfig, FileProgress, VersionProgress},
  consts::PULL_FILES_SIZE,
  handlers::dto::{DownloadProgress, DownloadStatus, UnzipTask},
  service::{files::ServiceFiles, get_release::ServiceGetRelease, main::Service, unpack::ServiceUnpacker},
  utils::errors::log_full_error,
};
use anyhow::Context;
use std::{
  collections::HashMap,
  fs,
  path::{Path, PathBuf},
  sync::{Arc, Mutex as StdMutex},
};
use std::{
  sync::atomic::{AtomicU16, Ordering},
  time::Duration,
};
use tauri::Emitter;
use tokio::sync::mpsc;
use tokio::sync::{Mutex, broadcast};

pub type CancelMap = Arc<StdMutex<HashMap<String, broadcast::Sender<()>>>>;

#[tauri::command]
pub async fn cancel_download_version(channel_map: tauri::State<'_, CancelMap>, releaseName: String) -> Result<(), String> {
  if let Some(tx) = channel_map.lock().unwrap().remove(&releaseName) {
    let _ = tx.send(());
  }

  Ok(())
}

#[tauri::command]
pub async fn start_download_version(
  app: tauri::AppHandle,
  channel_map: tauri::State<'_, CancelMap>,
  app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>,
  service: tauri::State<'_, Arc<Mutex<Service>>>,
  service_files: tauri::State<'_, Arc<ServiceFiles>>,
  service_unpack: tauri::State<'_, Arc<ServiceUnpacker>>,
  downloadPath: String,
  installPath: String,
  versionName: String,
  versionId: Option<u32>,
) -> Result<(), String> {
  let (tx, mut rx) = broadcast::channel::<()>(1);
  {
    channel_map.lock().unwrap().insert(versionName.clone(), tx.clone());
  }
  // Удаляем запись после завершения (успешного или нет)
  scopeguard::defer! {
    channel_map.lock().unwrap().remove(&versionName);
  };

  let cfg = app_config.lock().await.clone();

  let selected_version = cfg
    .versions
    .iter()
    .find(|v| {
      if v.name == versionName {
        return true;
      }
      if let Some(id) = versionId {
        return v.id == id;
      }

      return false;
    })
    .ok_or_else(|| anyhow::anyhow!("Version not found, versionName: {:?} versionId: {:?}", &versionName, &versionId))
    .map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

  log::info!("start_download_versions, selected_version: {:?}", &selected_version);

  let mut total_file_count: u16 = 0;

  if let Some(data) = &selected_version.manifest {
    total_file_count = data.files.len() as u16;
  };

  let mut version = VersionProgress {
    id: selected_version.id,
    name: selected_version.name.clone(),
    path: selected_version.path.clone(),
    installed_path: installPath.clone(),
    download_path: downloadPath.clone(),
    is_downloaded: false,
    files: HashMap::new(),
    downloaded_files_cnt: 0,
    total_file_count,
    manifest: selected_version.manifest.clone(),
  };

  let _ = app.emit(
    "download-version",
    DownloadProgress {
      version_name: version.name.clone(),
      status: DownloadStatus::Init,
      file: "".to_owned(),
      progress: 0.0,
      downloaded_files_cnt: 0,
      total_file_count: version.total_file_count,
    },
  );

  let download_dir = Path::new(&version.download_path);
  std::fs::create_dir_all(&download_dir)
    .with_context(|| format!("Failed to create output download directory: {:?}", download_dir))
    .map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

  let release = {
    let service_guard = service.lock().await;
    service_guard
      .get_main_release(&selected_version.name)
      .await
      .context("Failed to get main release files")
      .map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?
  };

  if rx.try_recv().is_ok() {
    log::info!("Download task '{}' was cancelled", &versionName);
    return Err("USER_CANCELLED".to_string());
  }

  version.total_file_count = release.assets.len() as u16;

  let _ = app.emit(
    "download-version",
    DownloadProgress {
      version_name: version.name.clone(),
      status: DownloadStatus::Init,
      file: "".to_owned(),
      progress: 0.0,
      downloaded_files_cnt: version.downloaded_files_cnt,
      total_file_count: version.total_file_count,
    },
  );

  for file in &release.assets {
    version.files.insert(
      file.name.clone(),
      FileProgress {
        id: file.name.clone(),
        download_link: file.download_link.clone(),
        name: file.name.clone(),
        is_downloaded: false,
        is_unpacked: false,
        size: 0,
        total_size: file.size,
      },
    );
  }

  if rx.try_recv().is_ok() {
    log::info!("Download task '{}' was cancelled", &versionName);
    return Err("USER_CANCELLED".to_string());
  }

  {
    let mut config_guard = app_config.lock().await;
    config_guard.progress_download.insert(version.name.clone(), version.clone());
    config_guard.save().map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;
  }

  let _ = app.emit(
    "download-version",
    DownloadProgress {
      version_name: version.name.clone(),
      status: DownloadStatus::DownloadFiles,
      file: "".to_owned(),
      progress: 0.0,
      downloaded_files_cnt: version.downloaded_files_cnt,
      total_file_count: version.total_file_count,
    },
  );

  let total_file_count = release.assets.len() as u16;
  let downloaded_cnt = Arc::new(AtomicU16::new(0));

  // Создаем канал для очереди задач
  // Запас емкости берем с запасом, чтобы влезли все файлы + возможные ретраи
  let (tx_queue, mut rx_queue) = mpsc::channel(total_file_count as usize + 100);

  // Заполняем очередь начальными файлами
  for file in release.assets {
    tx_queue.send(file).await.map_err(|e| e.to_string())?;
  }

  // Обертка для доступа к API клиенту
  let api_client = {
    let service_guard = service.lock().await;
    service_guard.api_client.clone()
  };

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

  let mut join_handles = Vec::new();
  let (cancel_tx, _) = broadcast::channel::<()>(1); // Локальный сигнал для воркеров

  // Вставляем основной tx в карту отмены
  channel_map.lock().unwrap().insert(versionName.clone(), cancel_tx.clone());

  let rx_queue_arc = Arc::new(Mutex::new(rx_queue));
  let tx_queue_arc = Arc::new(Mutex::new(tx_queue));
  let tx_unzip_arc = Arc::new(tx_unzip);
  let cancel_tx_arc = Arc::new(cancel_tx);

  // Запускаем фиксированное количество воркеров
  for worker_id in 0..PULL_FILES_SIZE {
    let app_c = app.clone();
    let app_config_c = app_config.inner().clone();
    let service_files_c = service_files.inner().clone();
    let api_client_c = api_client.clone();
    let version_name_c = versionName.clone();
    let version_install_path_c = version.installed_path.clone();
    let download_dir_c = download_dir.to_path_buf();
    let downloaded_cnt_c = downloaded_cnt.clone();

    let tx_unzip_c = tx_unzip_arc.clone();
    let rx_queue_c = rx_queue_arc.clone();
    let tx_queue_c = tx_queue_arc.clone();
    let cancel_tx_arc_c = cancel_tx_arc.clone();
    let mut stop_rx = cancel_tx_arc.subscribe();

    // Переменная для rx очереди (нужен Mutex, так как mpsc::Receiver не Thread-safe)
    // Но в данном случае мы просто передаем владение rx каждому воркеру через Arc/Mutex
    // или используем подход с одним циклом.
    let handle = tokio::spawn(async move {
      loop {
        let file_task = {
          let mut rx_lock = rx_queue_c.lock().await;

          tokio::select! {
              // Остановка если пришла отмена
              _ = stop_rx.recv() => break,
              // Очередь пуста, воркер завершает работу
              task = rx_lock.recv() => {
                  match task {
                      Some(t) => t,
                      None => break,
                  }
              }
          }
        };

        let file_path = download_dir_c.join(&file_task.name);
        let part_path = format!("{}.part", file_path.to_str().unwrap_or(""));

        // Читаем существующий прогресс для Range
        let seek_pos = if let Ok(content) = std::fs::read_to_string(&part_path) {
          content.trim().parse::<u64>().ok()
        } else {
          None
        };

        let mut local_cancel = cancel_tx_arc_c.subscribe();
        let res = service_files_c
          .download_blob_to_file(
            &api_client_c,
            &version_name_c,
            &file_task.download_link,
            &file_task.size,
            &file_path,
            &seek_pos,
            local_cancel,
          )
          .await;

        match res {
          Ok(_) => {
            // Успешно скачано
            let current = downloaded_cnt_c.fetch_add(1, Ordering::SeqCst) + 1;
            let _ = tx_unzip_c
              .send(UnzipTask {
                file_name: file_task.name.clone(),
                archive_path: file_path.clone(),
                destination_path: PathBuf::from(&version_install_path_c),
                is_latest: current >= total_file_count,
              })
              .await;

            update_config_and_emit(&app_c, &app_config_c, &version_name_c, &file_task.name, current, total_file_count).await;
            if current >= total_file_count {
              let _ = cancel_tx_arc_c.send(());
              break;
            }
          }
          Err(e) => {
            log::error!("Error downloading {}: {}. Retrying...", file_task.name, e);
            // Возвращаем в очередь
            let channel = tx_queue_c.lock().await;
            let _ = channel.send(file_task).await;
            tokio::time::sleep(Duration::from_secs(2)).await; // Пауза перед ретраем
          }
        }
      }
    });
    join_handles.push(handle);
  }

  // Важно: чтобы rx_queue закрылся, нужно дропнуть все tx_queue, кроме тех что в воркерах
  drop(tx_queue_arc);

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

// Вспомогательная функция для получения задач из очереди внутри select!
async fn update_config_and_emit(
  app: &tauri::AppHandle,
  config: &Arc<Mutex<AppConfig>>,
  version_name: &str,
  file_name: &str,
  current: u16,
  total: u16,
) {
  let mut config_guard = config.lock().await;
  if let Some(ver) = config_guard.progress_download.get_mut(version_name) {
    if let Some(file_progress) = ver.files.get_mut(file_name) {
      file_progress.is_downloaded = true;
    }
    ver.downloaded_files_cnt = current;
  }
  let _ = config_guard.save();

  let progress = (current as f32 / total as f32) * 100.0;
  let _ = app.emit(
    "download-version",
    DownloadProgress {
      version_name: version_name.to_string(),
      status: DownloadStatus::DownloadFiles,
      file: file_name.to_string(),
      progress,
      downloaded_files_cnt: current,
      total_file_count: total,
    },
  );
}
