use std::path::Path;

use crate::{configs::AppConfig::Version, consts::VERSIONS_DIR, providers::dto::TreeItem, service::main::Service};

use anyhow::{Result, bail};
use futures_util::future::join_all;
use regex::Regex;

pub trait ServiceGetRelease {
  async fn get_releases(&self) -> Result<Vec<Version>>;
  async fn get_main_release_files(&self, release_id: u32) -> Result<Vec<TreeItem>>;
  async fn get_local_version(&self) -> Result<Vec<Version>>;
  async fn set_release_visibility(&self, path: String, visibility: bool) -> Result<()>;
}

impl ServiceGetRelease for Service {
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
        is_local: false,
      })
      .collect();

    Ok(result)
  }

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

  async fn get_local_version(&self) -> Result<Vec<Version>> {
    let install_path = {
      let config_guard = self.config.lock().await;
      config_guard.install_path.clone()
    };
    let versions_dir = Path::new(&install_path).join(VERSIONS_DIR);

    let mut versions: Vec<Version> = vec![];

    for entry in std::fs::read_dir(&versions_dir)? {
      let entry = entry?;
      let path = entry.path();

      let key_path = entry.file_name().clone().into_string().expect("OsString was not valid UTF-8");
      let name = Regex::new(r"[-]+").unwrap().replace_all(&key_path, " ").to_string();

      log::info!(
        "Get local version, name {:?} path: {:?} file_name: {:?} entry: {:?}",
        &name,
        &path,
        &entry.file_name(),
        &entry
      );

      versions.push(Version {
        id: 0,
        name: name,
        path: key_path,
        installed_updates: vec![],
        is_local: true,
      });
    }

    Ok(versions)
  }

  async fn set_release_visibility(&self, path: String, visibility: bool) -> Result<()> {
    let api = self.api_client.current_provider()?;

    api.set_release_visibility(path, visibility).await?;

    Ok(())
  }
}
