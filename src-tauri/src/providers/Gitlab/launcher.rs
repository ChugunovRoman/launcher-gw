use crate::providers::{
  Gitlab::{Gitlab::Gitlab, files::__get_file_content_size, models::*},
  dto::{ReleaseAssetGit, ReleaseGit, ReleasePlatform},
};

use anyhow::{Context, Result, bail};

pub async fn __get_launcher_latest_release(s: &Gitlab, project_id: &str) -> Result<ReleaseGit> {
  let url = format!("{}/projects/{}/releases", &s.host, &project_id);
  let resp = s
    .get(&url)
    .send()
    .await
    .context("Failed to send request to GitLab (get_launcher_release)")?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("__get_launcher_latest_release, GitLab API error {}: {} url: {}", status, body, url);
  }

  let release: Vec<ReleaseGitlab> = resp.json().await.context("Failed to parse ReleaseGitlab response as JSON")?;

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
