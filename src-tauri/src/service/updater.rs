use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::consts::{BASE_DIR, GITHUB_LAUNCHER_REPO_NAME, MAIN_DEVELOPER_NAME, REPO_LAUNCGER_ID_2};
use crate::providers::ApiClient::ApiClient::ApiClient;
use crate::providers::dto::{ReleaseGit, ReleasePlatform};
use crate::utils::paths::get_exe_name;
use crate::utils::resources::launcher_exe;
use anyhow::{Context, Result, bail};
use futures_util::stream::StreamExt;
use semver::Version;
use tauri::Manager;
use tauri::path::BaseDirectory;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

pub type DownloadProgressCallback = Box<dyn Fn(&str, u64, u64) + Send + Sync>;

pub struct ServiceUpdater {
  callback: Arc<DownloadProgressCallback>,
  /// Exe path captured BEFORE self_replace renamed the running binary.
  /// `current_exe()` is unreliable after an update: on Windows the running exe
  /// is renamed to a temp name by self_replace, so restarting via
  /// `current_exe()` would spawn the old renamed binary again.
  original_exe: Mutex<Option<PathBuf>>,
}

impl ServiceUpdater {
  pub fn new<F>(callback: F) -> Self
  where
    F: Fn(&str, u64, u64) + Send + Sync + 'static,
  {
    Self {
      callback: Arc::new(Box::new(callback)),
      original_exe: Mutex::new(None),
    }
  }

  /// Path of the binary captured before the self-update replaced it.
  pub fn original_exe(&self) -> Option<PathBuf> {
    self.original_exe.lock().unwrap().clone()
  }

  pub async fn check(&self, api_client: &ApiClient, current_version: String) -> Result<Option<ReleaseGit>> {
    let api = api_client.current_provider()?;

    log::debug!("ServiceUpdater.check, start");

    let project_id = if api.is_suppot_subgroups() {
      REPO_LAUNCGER_ID_2.to_string()
    } else {
      GITHUB_LAUNCHER_REPO_NAME.to_string()
    };
    let latest_release = api.get_launcher_latest_release(MAIN_DEVELOPER_NAME, &project_id).await?;

    log::debug!("ServiceUpdater.check, latest_release.tag_name: {:?}", &latest_release.version);

    let current_v = Version::parse(&current_version).unwrap_or(Version::new(0, 0, 0));
    let latest_v = Version::parse(&latest_release.version).unwrap_or(Version::new(0, 0, 0));

    log::debug!(
      "ServiceUpdater.check, current_v: {} latest_v: {} need update: {}",
      &current_version,
      &latest_release.version,
      latest_v > current_v
    );

    if latest_v > current_v {
      return Ok(Some(latest_release));
    }

    Ok(None)
  }

  pub async fn download(&self, api_client: &ApiClient, app_handle: &tauri::AppHandle, release: ReleaseGit) -> Result<Option<PathBuf>> {
    let api = api_client.current_provider()?;

    log::debug!("ServiceUpdater.download, start");

    let mut asset_name = ReleasePlatform::Windows;

    if cfg!(target_os = "windows") {
      asset_name = ReleasePlatform::Windows;
    } else if cfg!(target_os = "macos") {
      asset_name = ReleasePlatform::MacOS;
    } else {
      asset_name = ReleasePlatform::Linux;
    }

    log::debug!("ServiceUpdater.download, asset_name: {:?}", &asset_name);

    if let Some(target) = release.assets.iter().find(|&asset| asset.platform == asset_name) {
      log::debug!("ServiceUpdater.download, target: {:?}", &target);

      let mut stream = api.get_blob_by_url_stream(&target.download_link, &None).await?;

      let base_dir = app_handle
        .path()
        .resolve(BASE_DIR, BaseDirectory::AppConfig)
        .context("Failed to resolve config directory")?
        .parent()
        .unwrap()
        .to_path_buf();
      let exe_name = get_exe_name().unwrap_or(launcher_exe());
      let file_path = base_dir.join(&exe_name);
      let mut file = File::create(&file_path).await.context("Failed to create output file")?;

      log::debug!("ServiceUpdater.download, start download file: {:?}", &target.download_link);
      let mut downloaded: u64 = 0;
      while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Error reading chunk from response stream")?;
        let chunk_len = chunk.len() as u64;

        file.write_all(&chunk).await.context("Failed to write chunk to file")?;
        downloaded += chunk_len;

        (self.callback)(&release.version, downloaded, target.size);
      }

      file.flush().await.context("Failed to flush launcher download")?;

      if target.size > 0 && downloaded != target.size {
        bail!(
          "Launcher download size mismatch: got {} bytes, expected {}",
          downloaded,
          target.size
        );
      }

      log::debug!("ServiceUpdater.download, finish download file: {:?}", &target.download_link);

      return Ok(Some(file_path));
    };

    log::debug!("ServiceUpdater.download, asset not found!, asset_name: {:?}", &asset_name);

    Ok(None)
  }

  pub async fn install(&self, file_path: PathBuf) -> Result<()> {
    // Replace the running binary with the downloaded file (atomic where supported).
    self_replace::self_replace(&file_path).context("self_replace error")?;
    Ok(())
  }

  pub async fn download_and_install(&self, api_client: &ApiClient, app_handle: &tauri::AppHandle, release: ReleaseGit) -> Result<bool> {
    if let Some(target) = self.download(api_client, app_handle, release).await? {
      // Capture the exe path BEFORE install(): self_replace renames the
      // running binary, making current_exe() point to the temp old file.
      *self.original_exe.lock().unwrap() = std::env::current_exe().ok();

      self.install(target).await?;

      return Ok(true);
    }

    Ok(false)
  }

  pub async fn restart(&self, app_handle: &tauri::AppHandle) -> Result<()> {
    // Graceful shutdown first (cancel downloads/uploads, flush config.json)
    // so the new instance does not race us for shared files.
    crate::handlers::window::graceful_shutdown(app_handle).await;

    // Never returns: spawns the replacement with the restart lock handshake
    // and exits the current process.
    crate::utils::restart::restart_launcher(app_handle, self.original_exe());
  }
}
