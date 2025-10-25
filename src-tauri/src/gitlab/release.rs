use crate::{
  configs::AppConfig::Version,
  gitlab::{
    Gitlab::Gitlab,
    files::GitLabFiles,
    models::{Group, TreeItem},
  },
};

use anyhow::{Context, Result, bail};
use futures_util::future::join_all;

pub trait GitLabRelease {
  async fn get_releases(&self) -> Result<Vec<Version>>;
  async fn get_repos(&self, group_id: u32) -> Result<Vec<Version>>;
  async fn get_main_release_files(&self, release_id: u32) -> Result<Vec<TreeItem>>;
}

impl GitLabRelease for Gitlab {
  async fn get_releases(&self) -> Result<Vec<Version>> {
    let url = format!("{}/groups/117668928/subgroups?sort=desc", self.host);
    let resp = self
      .client
      .get(&url)
      .send()
      .await
      .context("Failed to send request to GitLab (get_releases)")?;

    if resp.status().is_success() {
      let groups: Vec<Group> = resp
        .json()
        .await
        .context("Failed to parse GitLab groups response as JSON")?;

      let versions = groups
        .into_iter()
        .filter(|group| group.marked_for_deletion_on.is_none())
        .map(|group| Version {
          id: group.id,
          name: group.name,
          path: group.path,
          installed_updates: vec![],
        })
        .collect();

      Ok(versions)
    } else {
      let status = resp.status();
      let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
      bail!("GitLab API error {}: {}", status, body);
    }
  }

  async fn get_repos(&self, group_id: u32) -> Result<Vec<Version>> {
    let url = format!("{}/groups/{}/projects", self.host, group_id);
    let resp = self
      .client
      .get(&url)
      .send()
      .await
      .context("Failed to send request to GitLab (get_repos)")?;

    if resp.status().is_success() {
      let groups: Vec<Group> = resp
        .json()
        .await
        .context("Failed to parse GitLab projects response as JSON")?;

      let versions = groups
        .into_iter()
        .filter(|group| group.marked_for_deletion_on.is_none())
        .map(|group| Version {
          id: group.id,
          name: group.name,
          path: group.path,
          installed_updates: vec![],
        })
        .collect();

      Ok(versions)
    } else {
      let status = resp.status();
      let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
      bail!("GitLab API error {}: {}", status, body);
    }
  }

  async fn get_main_release_files(&self, release_id: u32) -> Result<Vec<TreeItem>> {
    let repos = self
      .get_repos(release_id)
      .await
      .context("Failed to fetch repos for release")?;

    let main_repos: Vec<_> = repos
      .iter()
      .filter(|repo| repo.path.starts_with("main_"))
      .collect();

    if main_repos.is_empty() {
      bail!("No 'main_' repos found for release {}", release_id);
    }

    let tasks: Vec<_> = main_repos
      .iter()
      .map(|repo| {
        log::info!("Fetching files from repo: {:?}", repo);
        self.get_all_files_in_repo(repo.id)
      })
      .collect();

    let results = join_all(tasks).await;

    let mut all_files = Vec::new();
    let mut errors = Vec::new();

    for (repo, result) in main_repos.iter().zip(results) {
      match result {
        Ok(mut files) => {
          for file in &mut files {
            file.project_id = repo.id;
          }
          all_files.extend(files); // перемещаем владение
        }
        Err(e) => {
          log::error!("Error fetching files from repo {}: {}", repo.id, e);
          errors.push(e);
        }
      }
    }

    if all_files.is_empty() {
      if let Some(first_err) = errors.into_iter().next() {
        return Err(first_err.into());
      } else {
        bail!("No files found and no specific error occurred");
      }
    }

    Ok(all_files)
  }
}
