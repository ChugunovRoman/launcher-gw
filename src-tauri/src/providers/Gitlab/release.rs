use crate::providers::{
  ApiProvider::ApiProvider,
  Gitlab::{Gitlab::Gitlab, models::*},
  dto::*,
};

use anyhow::{Context, Result, bail};

pub async fn __get_releases(s: &Gitlab) -> Result<Vec<Release>> {
  let root_id = s.get_manifest()?.root_id.context("Cannot get root_id from Gitlab manifest file!")?;

  let url = format!("{}/groups/{}/subgroups?sort=desc", &s.host, &root_id);
  let resp = s
    .get_client()
    .get(&url)
    .send()
    .await
    .context("Failed to send request to GitLab (get_releases)")?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("GitLab API error {}: {}", status, body);
  }

  let groups: Vec<Group> = resp.json().await.context("Failed to parse GitLab groups response as JSON")?;

  let versions = groups
    .into_iter()
    .filter(|group| group.marked_for_deletion_on.is_none())
    .map(|group| Release {
      id: group.id,
      name: group.name,
      path: group.path,
    })
    .collect();

  Ok(versions)
}

pub async fn __get_release_repos(s: &Gitlab, release_id: u32) -> Result<Vec<Project>> {
  let url = format!("{}/groups/{}/projects", &s.host, &release_id);
  let resp = s
    .get_client()
    .get(&url)
    .send()
    .await
    .context("Failed to send request to GitLab (get_repos)")?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("GitLab API error {}: {}", status, body);
  }

  let repos: Vec<ProjectGitlab> = resp.json().await.context("Failed to parse GitLab projects response as JSON")?;

  let versions = repos
    .into_iter()
    .filter(|repo: &ProjectGitlab| repo.marked_for_deletion_on.is_none() && repo.name.starts_with("main_"))
    .map(|repo| Project {
      id: repo.id,
      name: repo.name,
      path: repo.path,
      ssh_remote_url: repo.ssh_url_to_repo,
      marked_for_deletion_on: repo.marked_for_deletion_on,
    })
    .collect();

  Ok(versions)
}

pub async fn __get_updates_repos(s: &Gitlab, release_id: u32) -> Result<Vec<Project>> {
  let url = format!("{}/groups/{}/projects", &s.host, &release_id);
  let resp = s
    .get_client()
    .get(&url)
    .send()
    .await
    .context("Failed to send request to GitLab (get_repos)")?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("GitLab API error {}: {}", status, body);
  }

  let repos: Vec<ProjectGitlab> = resp.json().await.context("Failed to parse GitLab projects response as JSON")?;

  let versions = repos
    .into_iter()
    .filter(|repo: &ProjectGitlab| repo.marked_for_deletion_on.is_none() && repo.name.starts_with("updates_"))
    .map(|repo| Project {
      id: repo.id,
      name: repo.name,
      path: repo.path,
      ssh_remote_url: repo.ssh_url_to_repo,
      marked_for_deletion_on: repo.marked_for_deletion_on,
    })
    .collect();

  Ok(versions)
}
