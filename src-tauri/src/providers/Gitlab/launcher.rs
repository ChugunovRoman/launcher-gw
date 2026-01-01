use crate::providers::Gitlab::{Gitlab::Gitlab, models::*};

use anyhow::{Context, Result, bail};

pub async fn __get_launcher_latest_release(s: &Gitlab, project_id: u32) -> Result<ReleaseGitlab> {
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

  Ok(release[0].clone())
}
