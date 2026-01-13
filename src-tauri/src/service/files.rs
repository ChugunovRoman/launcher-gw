use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::providers::ApiClient::ApiClient::ApiClient;
use crate::utils::paths::get_file_name;
use anyhow::{Context, Result};
use futures_util::stream::StreamExt;
use std::io::SeekFrom;
use tokio::fs::OpenOptions;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::broadcast::Receiver;

pub type NetSpeedCallback = Box<dyn Fn(&str, &str, u64, u64, f64) + Send + Sync>;

pub struct ServiceFiles {
  callback: Arc<NetSpeedCallback>,
}

impl ServiceFiles {
  pub fn new<F>(callback: F) -> Self
  where
    F: Fn(&str, &str, u64, u64, f64) + Send + Sync + 'static,
  {
    Self {
      callback: Arc::new(Box::new(callback)),
    }
  }

  pub async fn get_launcher_bg(&self, api_client: &ApiClient) -> Result<Vec<u8>> {
    let api = api_client.current_provider()?;

    api.get_launcher_bg().await
  }

  pub async fn download_blob_to_file(
    &self,
    api_client: &ApiClient,
    release_name: &str,
    direct_url: &str,
    total_bytes: &u64,
    output_path: impl AsRef<Path>,
    seek: &Option<u64>,
    mut rx: Receiver<()>,
  ) -> Result<()> {
    let api = api_client.current_provider()?;
    let mut stream = api.get_blob_by_url_stream(&direct_url, seek).await?;

    let file_name = get_file_name(&output_path).unwrap();
    let part_file_path = format!("{}.part", output_path.as_ref().to_str().unwrap());

    // 1. Открываем основной файл правильно
    let mut file = OpenOptions::new().write(true).create(true).open(&output_path).await?;

    // Если мы докачиваем, перемещаем указатель в конец
    let mut downloaded: u64 = 0;
    if let Some(seek_pos) = seek {
      file.seek(SeekFrom::Start(*seek_pos)).await?;
      downloaded = *seek_pos;
    }

    let start_time = Instant::now();
    let mut last_callback = Instant::now();

    while let Some(chunk) = stream.next().await {
      // Проверка прерывания
      if let Ok(_) | Err(tokio::sync::broadcast::error::TryRecvError::Closed) = rx.try_recv() {
        log::info!("Download interrupted for file: {}", file_name);
        // Сохраняем прогресс перед выходом
        Self::save_part_file(&part_file_path, downloaded).await?;
        return Ok(());
      }

      let chunk = chunk.context("Error reading chunk from response stream")?;
      let chunk_len = chunk.len() as u64;

      file.write_all(&chunk).await.context("Failed to write chunk to file")?;
      downloaded += chunk_len;

      let now = Instant::now();
      if now.duration_since(last_callback) >= Duration::from_millis(100) {
        // 2. Периодически сохраняем прогресс в .part файл
        Self::save_part_file(&part_file_path, downloaded).await?;

        let elapsed = now.duration_since(start_time).as_secs_f64();
        let speed = if elapsed > 0.0 {
          (downloaded - seek.unwrap_or(0)) as f64 / elapsed
        } else {
          0.0
        };

        (self.callback)(release_name, &file_name, downloaded, total_bytes.clone(), speed);
        last_callback = now;
      }
    }

    file.flush().await?;

    // 3. После успешного завершения удаляем .part файл
    let _ = tokio::fs::remove_file(&part_file_path).await;

    (self.callback)(release_name, &file_name, downloaded, total_bytes.clone(), 0.);
    Ok(())
  }

  // Вспомогательная функция для записи прогресса
  async fn save_part_file(path: &str, downloaded: u64) -> Result<()> {
    // Записываем число как строку, это надежнее и проще для отладки
    tokio::fs::write(path, downloaded.to_string().as_bytes()).await?;
    Ok(())
  }
}
