use crate::{configs::AppConfig::Version, providers::dto::TreeItem, service::main::Service};

use anyhow::{Result, bail};
use futures_util::future::join_all;

pub trait ServiceRelease {
  async fn get_releases(&self) -> Result<Vec<Version>>;
  // async fn get_repos(&self, group_id: u32) -> Result<Vec<Version>>;
  async fn get_main_release_files(&self, release_id: u32) -> Result<Vec<TreeItem>>;
}

impl ServiceRelease for Service {
  async fn get_releases(&self) -> Result<Vec<Version>> {
    let api = self.api_client.current_provider()?;
    let releases = api.get_releases().await?;

    let result = releases
      .iter()
      .map(|release| Version {
        id: release.id.clone(),
        name: release.name.clone(),
        path: release.path.clone(),
        installed_updates: vec![],
      })
      .collect();

    Ok(result)
  }

  // async fn get_repos(&self, group_id: u32) -> Result<Vec<Version>> {
  //   let url = format!("{}/groups/{}/projects", self.host, group_id);
  //   let resp = self
  //     .client
  //     .get(&url)
  //     .send()
  //     .await
  //     .context("Failed to send request to GitLab (get_repos)")?;

  //   if resp.status().is_success() {
  //     let repos: Vec<Group> = resp.json().await.context("Failed to parse GitLab projects response as JSON")?;

  //     let versions = repos
  //       .into_iter()
  //       .filter(|repo: &Group| repo.marked_for_deletion_on.is_none())
  //       .map(|repo| Version {
  //         id: repo.id,
  //         name: repo.name,
  //         path: repo.path,
  //         installed_updates: vec![],
  //       })
  //       .collect();

  //     Ok(versions)
  //   } else {
  //     let status = resp.status();
  //     let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
  //     bail!("GitLab API error {}: {}", status, body);
  //   }
  // }

  async fn get_main_release_files(&self, release_id: u32) -> Result<Vec<TreeItem>> {
    let api = self.api_client.current_provider()?;

    let repos = api.get_release_repos(release_id).await?;

    if repos.is_empty() {
      bail!("No 'main_' repos found for release {}", release_id);
    }

    let tasks: Vec<_> = repos
      .iter()
      .map(|repo| {
        log::info!("Fetching files from repo: {:?}", repo);
        api.get_full_tree(repo.id)
      })
      .collect();

    let results = join_all(tasks).await;

    let mut all_files = Vec::new();
    let mut errors = Vec::new();

    for (repo, result) in repos.iter().zip(results) {
      match result {
        Ok(files) => {
          all_files.extend(files);
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
