use bytes::Bytes;
use futures_util::Stream;
use regex::Regex;
use std::time::Instant;
use std::{fs, path::Path, sync::Arc};
use tauri::{Emitter, Manager};
use tokio::{fs::File, sync::Mutex};
use tokio_util::io::ReaderStream;

use crate::{
  consts::MANIFEST_NAME,
  handlers::dto::{ReleaseManifest, UploadProgressPayload},
  providers::dto::CreateReleaseAsset,
  service::{get_release::ServiceGetRelease, main::Service},
  utils::errors::{log_full_error, upload_log},
};

#[tauri::command]
pub async fn upload_v2_release(app: tauri::AppHandle, name: String, path: String) -> Result<(), String> {
  let base_dir = Path::new(&path);

  let manifest_path = base_dir.join(MANIFEST_NAME);
  let manifest_content = fs::read_to_string(manifest_path).map_err(|e| e.to_string())?;
  let manifest_release: ReleaseManifest = serde_json::from_str(&manifest_content).map_err(|e| e.to_string())?;

  let _ = app.emit("upload-progress-get-manifest", &manifest_release);

  let manifest = {
    let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
    let service_guard = state.lock().await;
    let api = service_guard.api_client.current_provider().map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

    api.get_manifest()
  }
  .map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  upload_log(&app, format!("Start upload_release, max_size: {} path: {}", &manifest.max_size, &path));

  let releases = {
    let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
    let mut service_guard = state.lock().await;

    service_guard.get_releases(false).await
  }
  .map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  let release = releases
    .iter()
    .find(|r| r.name == name)
    .expect(&format!("upload_release(), Release by name {} not found !", &name));

  upload_log(&app, format!("Found release: {} ({})", &release.name, &release.id));

  let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
  let service_guard = state.lock().await;

  let api = service_guard.api_client.current_provider().map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  let main_repos = api.get_release_repos_by_name(&release.name).await.map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  let project = &main_repos[0];
  let project_id = if api.is_suppot_subgroups() {
    project.id.to_string()
  } else {
    project.name.clone()
  };

  upload_log(&app, format!("Creating {} in repo: {:?}", MANIFEST_NAME, project));

  api
    .add_file_to_repo(&project_id, MANIFEST_NAME, &manifest_content, "Upload manifest.json", "master")
    .await
    .map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

  upload_log(&app, format!("File {} upload successful !", MANIFEST_NAME));

  let tag_name = Regex::new(r"\s+").unwrap().replace_all(&name, "-").to_string();

  api.create_tag(&project_id, &tag_name, "master").await.map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  upload_log(&app, format!("Tag {} created successful !", tag_name));

  let namespace = "gw_releases".to_owned();
  let asset_base_url = api.get_asset_url();
  let mut assets = manifest_release
    .files
    .iter()
    .map(|file| {
      let url = asset_base_url
        .replace("<PROJECT_ID>", &project_id)
        .replace("<NAME_SPACE>", &namespace)
        .replace("<VERSION>", &tag_name)
        .replace("<FILE_NAME>", &file.name);

      CreateReleaseAsset {
        file_name: file.name.clone(),
        file_download_url: url,
      }
    })
    .collect();

  let created_release = api.create_release(&project_id, &tag_name, assets).await.map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  assets = manifest_release
    .files
    .iter()
    .map(|file| {
      let url = created_release
        .upload_url
        .replace("<PROJECT_ID>", &project_id)
        .replace("<NAME_SPACE>", &namespace)
        .replace("<VERSION>", &tag_name)
        .replace("<FILE_NAME>", &file.name);

      log::debug!("Second change assset: {} url: {}", &file.name, &url);

      CreateReleaseAsset {
        file_name: file.name.clone(),
        file_download_url: url,
      }
    })
    .collect();

  upload_log(&app, format!("Release for {} created successful !", tag_name));

  for asset in assets {
    let app_handle = app.clone();
    // Клонируем данные ассета, так как они тоже понадобятся внутри стрима
    let asset_name = asset.file_name.clone();

    // 1. Получаем исходный стрим (например, из файла)
    let file = File::open(&base_dir.join(&asset_name)).await.map_err(|e| e.to_string())?;
    let total_size = file.metadata().await.map_err(|e| e.to_string())?.len();
    let file_stream = ReaderStream::new(file);
    let start_time = Instant::now();

    // 2. Создаем стрим с прогрессом
    let progress_stream = async_stream::stream! {
        let mut uploaded = 0;
        for await chunk in file_stream {
            if let Ok(ref data) = chunk {
                uploaded += data.len() as u64;
                // Вычисляем прошедшее время
            let elapsed = start_time.elapsed().as_secs_f64();

            // Вычисляем скорость (байты / секунды)
            // Добавляем проверку на 0, чтобы избежать деления на ноль в самом начале
            let speed = if elapsed > 0.0 {
                uploaded as f64 / elapsed
            } else {
                0.0
            };

                let _ = app_handle.emit("upload-progress", UploadProgressPayload {
                  file_name: asset_name.clone(),
                  file_uploaded_size: uploaded,
                  file_total_size: total_size,
                  total_uploaded_size: uploaded,
                  total_size: total_size,
                  speed
                });
            }
            yield chunk;
        }
    };

    // 3. Боксируем и передаем в провайдер
    let boxed_stream: Box<dyn Stream<Item = std::io::Result<Bytes>> + Send + Unpin> = Box::new(Box::pin(progress_stream));

    log::debug!("Try upload assset: {} by url: {}", &asset.file_name, &asset.file_download_url);

    api
      .upload_release_file(&asset.file_download_url, total_size, boxed_stream)
      .await
      .map_err(|e| e.to_string())?;

    upload_log(&app, format!("File {} uploaded successful !", &asset.file_name));
  }

  service_guard.set_release_visibility(&release.name, true).await.map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  upload_log(&app, "FULL Upload completed successful !".to_string());

  log::info!("Full upload of version {} finish successful !", &name);

  Ok(())
}
