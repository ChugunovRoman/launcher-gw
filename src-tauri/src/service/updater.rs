use std::fs;
use std::path::PathBuf;
use std::process::Command;

use crate::consts::{BASE_DIR, GITLAB_PID, REPO_LAUNCGER_ID_2};
use crate::providers::ApiClient::ApiClient::ApiClient;
use crate::providers::Gitlab::models::ReleaseGitlab;
use crate::utils::paths::get_exe_name;
use crate::utils::resources::launcher_exe;
use anyhow::{Context, Result};
use futures_util::stream::StreamExt;
use semver::Version;
use tauri::Manager;
use tauri::path::BaseDirectory;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

pub struct ServiceUpdater {}

impl ServiceUpdater {
  pub fn new() -> Self {
    Self {}
  }

  pub async fn check(&self, api_client: &ApiClient, current_version: String) -> Result<Option<ReleaseGitlab>> {
    let api = api_client.get_provider(GITLAB_PID)?;

    log::debug!("ServiceUpdater.check, start");

    let latest_release = api.get_launcher_latest_release(REPO_LAUNCGER_ID_2).await?;

    log::debug!("ServiceUpdater.check, latest_release.tag_name: {:?}", &latest_release.tag_name);

    let current_v = Version::parse(&current_version).unwrap_or(Version::new(0, 0, 0));
    let latest_v = Version::parse(&latest_release.tag_name).unwrap_or(Version::new(0, 0, 0));

    log::debug!(
      "ServiceUpdater.check, current_v: {} latest_v: {} need update: {}",
      &current_version,
      &latest_release.tag_name,
      latest_v > current_v
    );

    if latest_v > current_v {
      return Ok(Some(latest_release));
    }

    Ok(None)
  }

  pub async fn download(&self, api_client: &ApiClient, app_handle: &tauri::AppHandle, release: ReleaseGitlab) -> Result<Option<PathBuf>> {
    let api = api_client.get_provider(GITLAB_PID)?;

    log::debug!("ServiceUpdater.download, start");

    let mut asset_name = "".to_owned();

    if cfg!(target_os = "windows") {
      asset_name = "Windows".to_owned();
    } else if cfg!(target_os = "macos") {
      asset_name = "MacOS".to_owned();
    } else {
      asset_name = "Linux".to_owned();
    }

    log::debug!("ServiceUpdater.download, asset_name: {}", &asset_name);

    if let Some(target) = release.assets.links.iter().find(|&link| link.name == asset_name) {
      log::debug!("ServiceUpdater.download, target: {:?}", &target);

      let mut stream = api.get_blob_by_url_stream(&target.direct_asset_url).await?;

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

      log::debug!("ServiceUpdater.download, start download file: {:?}", &target.direct_asset_url);
      while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Error reading chunk from response stream")?;

        file.write_all(&chunk).await.context("Failed to write chunk to file")?;
      }

      log::debug!("ServiceUpdater.download, finish download file: {:?}", &target.direct_asset_url);

      return Ok(Some(file_path));
    };

    log::debug!("ServiceUpdater.download, asset not found!, asset_name: {}", &asset_name);

    Ok(None)
  }

  pub async fn install(&self, file_path: PathBuf) -> Result<()> {
    let exe_path = std::env::current_exe().context("Cannot get current exe path")?;
    let bytes = fs::read(&file_path)?;

    let _ = self_replace::self_replace(&exe_path).context("self_replace error");

    fs::write(&exe_path, bytes).context("Cannot write launcher binary file!")?;

    Ok(())
  }

  pub async fn download_and_install(&self, api_client: &ApiClient, app_handle: &tauri::AppHandle, release: ReleaseGitlab) -> Result<bool> {
    if let Some(target) = self.download(api_client, app_handle, release).await? {
      self.install(target).await?;

      return Ok(true);
    }

    Ok(false)
  }

  pub async fn restart(&self) -> Result<()> {
    let exe_path = std::env::current_exe().context("Cannot get current exe path")?;

    Command::new(exe_path).spawn().context("Cannot resatrt the app")?;

    std::process::exit(0);
  }
}
