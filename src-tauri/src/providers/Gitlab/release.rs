use crate::providers::{
  ApiProvider::ApiProvider,
  Gitlab::{Gitlab::Gitlab, group::__update_group, models::*, repo::__update_repo},
  dto::*,
};

use anyhow::{Context, Result, bail};

pub async fn __get_releases(s: &Gitlab, cashed: bool) -> Result<Vec<Release>> {
  let root_id = s.get_manifest()?.root_id.context("Cannot get root_id from Gitlab manifest file!")?;

  let url = format!("{}/groups/{}/subgroups?sort=desc", &s.host, &root_id);
  let resp = s.get(&url).send().await.context("Failed to send request to GitLab (get_releases)")?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("__get_releases, GitLab API error {}: {} url: {}", status, body, url);
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

pub async fn __get_release_repos_by_name(s: &Gitlab, release_name: &str) -> Result<Vec<Project>> {
  let releases = __get_releases(s, true).await?;
  let release = releases
    .iter()
    .find(|r| r.name == release_name)
    .expect(&format!("get_release_repos_by_name(), relese with name: {} not found !", &release_name));

  let repos = __get_release_repos(s, &release.id.to_string()).await?;

  Ok(repos)
}

async fn __get_release_repos(s: &Gitlab, release_id: &str) -> Result<Vec<Project>> {
  let url = format!("{}/groups/{}/projects", &s.host, &release_id);
  let resp = s.get(&url).send().await.context("Failed to send request to GitLab (get_repos)")?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("__get_release_repos, GitLab API error {}: {} url: {}", status, body, url);
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

pub async fn __get_updates_repos_by_name(s: &Gitlab, release_id: &str) -> Result<Vec<Project>> {
  let url = format!("{}/groups/{}/projects", &s.host, &release_id);
  let resp = s.get(&url).send().await.context("Failed to send request to GitLab (get_repos)")?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("__get_updates_repos, GitLab API error {}: {} url: {}", status, body, url);
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

pub async fn __set_release_visibility(s: &Gitlab, release_id: &str, visibility: bool) -> Result<()> {
  let releases = __get_releases(s, true).await?;
  let release_id = match releases.iter().find(|r| r.path == release_id) {
    Some(data) => data.id,
    None => {
      bail!("set_release_visibility(), Release by path: {} not found !", release_id)
    }
  };

  let url = format!("{}/groups/{}/projects", &s.host, &release_id);
  let resp = s
    .get(&url)
    .send()
    .await
    .context("set_release_visibility(), Failed to send request to GitLab (get_repos)")?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("set_release_visibility(), GitLab API error {}: {}", status, body);
  }

  let repos: Vec<ProjectGitlab> = resp
    .json()
    .await
    .context("set_release_visibility(), Failed to parse GitLab projects response as JSON")?;

  if visibility {
    __update_group(
      s,
      &release_id.to_string(),
      UpdateGroupDtoGitlab {
        visibility: if visibility {
          Some(Visibility::Public)
        } else {
          Some(Visibility::Private)
        },
        ..UpdateGroupDtoGitlab::default()
      },
    )
    .await?;
  }
  for repo in repos {
    let _ = __update_repo(
      s,
      &repo.id.to_string(),
      UpdateRepoDtoGitlab {
        visibility: if visibility {
          Some(Visibility::Public)
        } else {
          Some(Visibility::Private)
        },
        ..UpdateRepoDtoGitlab::default()
      },
    )
    .await?;
  }

  if !visibility {
    __update_group(
      s,
      &release_id.to_string(),
      UpdateGroupDtoGitlab {
        visibility: if visibility {
          Some(Visibility::Public)
        } else {
          Some(Visibility::Private)
        },
        ..UpdateGroupDtoGitlab::default()
      },
    )
    .await?;
  }

  Ok(())
}
