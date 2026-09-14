use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::consts::{BASE_DIR, GITHUB_LAUNCHER_REPO_NAME, LAUNCHER_SHA256_MISMATCH, MAIN_DEVELOPER_NAME, REPO_LAUNCGER_ID_2};
use crate::providers::ApiClient::ApiClient::ApiClient;
use crate::providers::dto::{ReleaseAssetGit, ReleaseGit, ReleasePlatform};
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
    crate::utils::locks::lock(&self.original_exe).clone()
  }

  pub async fn check(&self, api_client: &ApiClient, current_version: String) -> Result<Option<ReleaseGit>> {
    log::debug!("ServiceUpdater.check, start");

    // Try the static release index first (0 API calls).
    let provider_id = api_client.current_provider()?.id();
    if let Ok(index) = crate::service::index::load_index(provider_id).await {
      log::debug!("ServiceUpdater.check, launcher version from index: {}", &index.launcher.version);
      let current_v = Version::parse(&current_version).unwrap_or(Version::new(0, 0, 0));
      let latest_v = Version::parse(&index.launcher.version).unwrap_or(Version::new(0, 0, 0));

      if latest_v > current_v {
        let assets: Vec<ReleaseAssetGit> = index
          .launcher
          .assets
          .iter()
          .map(|a| ReleaseAssetGit {
            name: a.name.clone(),
            platform: parse_platform(&a.platform),
            size: a.size,
            download_link: a.url.clone(),
          })
          .collect();
        return Ok(Some(ReleaseGit {
          name: "Launcher".to_string(),
          version: index.launcher.version,
          assets,
        }));
      }
      return Ok(None);
    }

    // Fallback: original API path.
    let api = api_client.current_provider()?;

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

  /// Expected SHA-256 of a launcher asset as published in the release index.
  ///
  /// The hash is looked up by asset name and only when the index describes the
  /// very version being downloaded — a stale index must never be used to judge
  /// a newer binary. Any failure (no index, old schema without `sha256`,
  /// unknown asset) yields None: the caller then falls back to the size check,
  /// exactly as before.
  async fn expected_sha256(&self, api_client: &ApiClient, version: &str, asset_name: &str) -> Option<String> {
    let provider_id = api_client.current_provider().ok()?.id();
    let index = crate::service::index::load_index(provider_id).await.ok()?;

    if index.launcher.version != version {
      log::warn!(
        "ServiceUpdater.download, index launcher version '{}' != downloaded '{}', skipping sha256 check",
        &index.launcher.version,
        version
      );
      return None;
    }

    let sha = index
      .launcher
      .assets
      .iter()
      .find(|a| a.name == asset_name)
      .and_then(|a| a.sha256.clone());

    if sha.is_none() {
      log::warn!("ServiceUpdater.download, no sha256 in index for asset '{}', size check only", asset_name);
    }

    sha
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

      let (mut stream, _stream_start) = api.get_blob_by_url_stream(&target.download_link, &None).await?;

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
      // Close the handle before hashing/deleting: on Windows an open handle
      // makes `remove_file` fail, which would leave a rejected binary on disk.
      drop(file);

      // The downloaded file replaces the running launcher, and there is no
      // second copy to fall back on, so accept it only when the size is both
      // KNOWN and exact.  A provider/proxy answering 200 with an HTML error
      // page used to pass straight through when `size` was 0.
      if target.size == 0 {
        let _ = tokio::fs::remove_file(&file_path).await;
        bail!("Launcher asset {:?} has no size in the index, refusing to install it", &asset_name);
      }
      if downloaded != target.size {
        let _ = tokio::fs::remove_file(&file_path).await;
        bail!(
          "Launcher download size mismatch: got {} bytes, expected {}",
          downloaded,
          target.size
        );
      }

      // The size is a weak guarantee (a truncated/substituted body of the same
      // length passes it), so verify the digest published in the index whenever
      // it is there. Old indexes carry no hash — behaviour stays size-only.
      if let Some(expected) = self.expected_sha256(api_client, &release.version, &target.name).await {
        let hash_path = file_path.clone();
        let actual = tokio::task::spawn_blocking(move || crate::utils::hash::sha256_file(&hash_path, None, None))
          .await
          .context("Failed to join launcher hash task")?;

        match actual {
          Ok(actual) => {
            if !actual.eq_ignore_ascii_case(expected.trim()) {
              let _ = tokio::fs::remove_file(&file_path).await;
              bail!(
                "{}: got {}, expected {}",
                LAUNCHER_SHA256_MISMATCH,
                actual,
                expected.trim()
              );
            }
            log::info!("ServiceUpdater.download, sha256 verified for '{}'", &target.name);
          }
          Err(e) => {
            let _ = tokio::fs::remove_file(&file_path).await;
            bail!("{}: {}", LAUNCHER_SHA256_MISMATCH, e);
          }
        }
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
      *crate::utils::locks::lock(&self.original_exe) = std::env::current_exe().ok();

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

fn parse_platform(s: &str) -> ReleasePlatform {
  match s {
    "windows" => ReleasePlatform::Windows,
    "linux" => ReleasePlatform::Linux,
    _ => ReleasePlatform::MacOS,
  }
}
