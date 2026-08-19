use std::time::Duration;

use crate::{
  consts::*,
  providers::{
    Github::{Github::Github, models::*},
    dto::{ReleaseAssetGit, ReleaseGit, ReleasePlatform},
  },
  utils::http_cache,
};

use anyhow::{Context, Result, bail};

pub async fn __get_launcher_latest_release(s: &Github, owner: &str, project_id: &str) -> Result<ReleaseGit> {
  let url = format!("{}/repos/{}/{}/releases/latest", &s.host, owner, project_id);
  let cached = s.get_cached(&url, Duration::from_secs(CACHE_TTL_RELEASE_SECS)).await?;
  let release: ReleaseGithub = serde_json::from_slice(&cached.bytes)
    .context("Failed to parse ReleaseGithub response as JSON")?;

  let mut assets: Vec<ReleaseAssetGit> = vec![];

  for asset in &release.assets {
    assets.push(ReleaseAssetGit {
      name: asset.name.clone(),
      platform: get_platform_type(&asset.name),
      size: asset.size,
      download_link: asset.browser_download_url.clone(),
    });
  }

  Ok(ReleaseGit {
    name: release.name,
    version: release.tag_name,
    assets: assets,
  })
}

fn get_platform_type(asset_name: &str) -> ReleasePlatform {
  if asset_name == EXE_WIN_NAME {
    ReleasePlatform::Windows
  } else if asset_name == EXE_LINUX_NAME {
    ReleasePlatform::Linux
  } else {
    ReleasePlatform::MacOS
  }
}
