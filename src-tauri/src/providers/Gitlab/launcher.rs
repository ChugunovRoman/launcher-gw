use std::time::Duration;

use crate::{
  consts::CACHE_TTL_RELEASE_SECS,
  providers::{
    Gitlab::{Gitlab::Gitlab, files::__get_file_content_size, models::*},
    dto::{ReleaseAssetGit, ReleaseGit, ReleasePlatform},
  },
  utils::http_cache,
};

use anyhow::{Context, Result, bail};

pub async fn __get_launcher_latest_release(s: &Gitlab, owner: &str, project_id: &str) -> Result<ReleaseGit> {
  let url = format!("{}/projects/{}/releases", &s.host, &project_id);
  let cached = s.get_cached(&url, Duration::from_secs(CACHE_TTL_RELEASE_SECS)).await?;
  let release: Vec<ReleaseGitlab> = serde_json::from_slice(&cached.bytes)
    .context("Failed to parse ReleaseGitlab response as JSON")?;

  if release.len() == 0 {
    bail!("There is not launcher releases in {} porject!", project_id);
  }

  let mut assets: Vec<ReleaseAssetGit> = vec![];

  for asset in &release[0].assets.links {
    let size = __get_file_content_size(s, &asset.direct_asset_url).await?;

    assets.push(ReleaseAssetGit {
      name: asset.name.clone(),
      size,
      platform: get_platform_type(&asset.name),
      download_link: asset.direct_asset_url.clone(),
    });
  }

  Ok(ReleaseGit {
    name: release[0].name.clone(),
    version: release[0].tag_name.clone(),
    assets: assets,
  })
}

fn get_platform_type(asset_name: &str) -> ReleasePlatform {
  if asset_name == "Windows" {
    ReleasePlatform::Windows
  } else if asset_name == "Linux" {
    ReleasePlatform::Linux
  } else {
    ReleasePlatform::MacOS
  }
}
