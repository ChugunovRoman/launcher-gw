use std::{fs, path::Path};

use crate::{
  configs::AppConfig::Version,
  consts::*,
  handlers::dto::ReleaseManifest,
  providers::dto::{Release, ReleaseGit, TreeItem},
  service::main::Service,
  utils::{encoding::read_cp1251_file, resources::game_exe},
};

use anyhow::{Result, bail};
use futures_util::future::join_all;
use regex::Regex;

pub trait ServiceGetRelease {
  async fn get_releases(&mut self, cashed: bool) -> Result<Vec<Version>>;
  async fn get_release_manifest(&self, release_name: &str) -> Result<ReleaseManifest>;
  async fn get_main_release_files(&self, release_id: &str) -> Result<Vec<TreeItem>>;
  async fn get_main_release(&self, release_name: &str) -> Result<ReleaseGit>;
  async fn get_local_version(&self) -> Result<Vec<Version>>;
  async fn get_main_version(&self) -> Option<Version>;
  async fn set_release_visibility(&self, path: &str, visibility: bool) -> Result<()>;
}

impl ServiceGetRelease for Service {
  async fn get_releases(&mut self, cashed: bool) -> Result<Vec<Version>> {
    let api = self.api_client.current_provider()?;
    let mut releases: Vec<Release> = vec![];

    if cashed && let Some(cash) = self.releases.get(api.id()) {
      releases = cash.clone();
    } else {
      releases = api.get_releases(cashed).await?;

      self.releases.insert(String::from(api.id()), releases.clone());
    }

    let result = releases
      .iter()
      .map(|release| Version {
        id: release.id.clone(),
        name: release.name.clone(),
        path: release.path.clone(),
        manifest: None,
        engine_path: None,
        fsgame_path: None,
        userltx_path: None,
        installed_path: "".to_owned(),
        download_path: "".to_owned(),
        installed_updates: vec![],
        is_local: false,
      })
      .collect();

    Ok(result)
  }

  async fn get_release_manifest(&self, release_name: &str) -> Result<ReleaseManifest> {
    let api = self.api_client.current_provider()?;
    let repos = api.get_release_repos_by_name(release_name.clone()).await?;

    let project = repos
      .iter()
      .find(|r| r.name.starts_with("main_1") || r.name.ends_with("main_1"))
      .expect(&format!("Repo main_1 not found for release: {}", &release_name));

    let project_id = if api.is_suppot_subgroups() {
      project.id.to_string()
    } else {
      project.name.clone()
    };
    let bytes = api.get_file_raw(&project_id, MANIFEST_NAME).await?;
    let manifest: ReleaseManifest = serde_json::from_slice(&bytes)?;

    Ok(manifest)
  }

  async fn get_main_release(&self, release_name: &str) -> Result<ReleaseGit> {
    let api = self.api_client.current_provider()?;

    let repos = api.get_release_repos_by_name(release_name).await?;

    if repos.is_empty() {
      bail!("No 'main_' repos found for release {}", release_name);
    }

    let main_repo = repos
      .iter()
      .find(|r| r.name.contains("_main_1"))
      .expect(&format!("Repo main_1 not found for release: {}", &release_name));

    let project_id = if api.is_suppot_subgroups() {
      main_repo.id.to_string()
    } else {
      main_repo.name.clone()
    };

    api.get_launcher_latest_release(GITHUB_ORG, &project_id).await
  }

  async fn get_main_release_files(&self, release_name: &str) -> Result<Vec<TreeItem>> {
    let api = self.api_client.current_provider()?;

    let repos = api.get_release_repos_by_name(release_name).await?;

    if repos.is_empty() {
      bail!("No 'main_' repos found for release {}", release_name);
    }

    let tasks: Vec<_> = repos
      .iter()
      .map(|repo| {
        let project_id = if api.is_suppot_subgroups() {
          repo.id.to_string()
        } else {
          repo.name.clone()
        };

        log::info!("Fetching files from repo: {:?}", repo);
        api.get_full_tree(project_id)
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
      config_guard.default_installed_path.clone()
    };
    let progress_download = {
      let config_guard = self.config.lock().await;
      config_guard.progress_download.clone()
    };
    let versions_dir = Path::new(&install_path);

    let mut versions: Vec<Version> = vec![];

    if !versions_dir.exists() {
      return Ok(versions);
    }

    for entry in std::fs::read_dir(&versions_dir)? {
      let entry = entry?;
      let path = entry.path();

      if path.is_file() {
        continue;
      }

      let bin_path = path.join(BIN_DIR);
      if !bin_path.exists() {
        continue;
      }

      let engine_path = bin_path.join(game_exe());
      if !engine_path.exists() {
        continue;
      }

      let key_path = entry.file_name().clone().into_string().expect("OsString was not valid UTF-8");
      let name = Regex::new(r"[-]+").unwrap().replace_all(&key_path, " ").to_string();

      if let Some(_) = progress_download.iter().find(|(_, progress)| progress.path == key_path) {
        continue;
      };

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
        manifest: None,
        engine_path: None,
        fsgame_path: None,
        userltx_path: None,
        installed_path: path.to_string_lossy().to_string(),
        download_path: path.to_string_lossy().to_string(),
        installed_updates: vec![],
        is_local: true,
      });
    }

    Ok(versions)
  }

  async fn get_main_version(&self) -> Option<Version> {
    let current_path = {
      let config_guard = self.config.lock().await;
      Path::new(&config_guard.install_path).to_owned()
    };

    let bin_path = current_path.join(BIN_DIR);
    let exe_path = bin_path.join(game_exe());
    let gamedata_path = current_path.join(GAMEDATA_DIR);
    let scripts_path = gamedata_path.join(SCRIPTS_DIR);
    let g_script_path = scripts_path.join(SCRIPT_G);
    let mut name = "[UNKNOWN]".to_owned();

    if !bin_path.exists() || !exe_path.exists() {
      return None;
    }

    if gamedata_path.exists() && scripts_path.exists() && g_script_path.exists() {
      let content = match fs::read_to_string(&g_script_path) {
        Ok(c) => c,
        Err(e) => {
          log::warn!("Cannot read _g.script as utf-8 file, error: {}", e);
          log::warn!("Start to read _g.script as cp1251 file...");
          match read_cp1251_file(&g_script_path) {
            Ok(c) => c,
            Err(e) => {
              log::error!("Error read _g.script as cp1251 file, error: {}", e);
              String::from("")
            }
          }
        }
      };

      let version = content.lines().find_map(|line| {
        let trimmed = line.trim();

        if trimmed.starts_with("VERSION =") || trimmed.starts_with("GAME_VERSION =") {
          trimmed
            .split('=')
            .nth(1)
            .map(|value| value.trim().trim_matches('"').split("..").next().unwrap_or("").trim().to_string())
        } else {
          None
        }
      });

      if let Some(line) = version {
        name = line;
      } else {
        log::warn!("Main game version not found in the _g.script file!");
      }
    }

    Some(Version {
      id: 0,
      name: name.clone(),
      path: name.replace(" ", "_"),
      manifest: None,
      engine_path: None,
      fsgame_path: None,
      userltx_path: None,
      installed_path: current_path.to_string_lossy().to_string(),
      download_path: current_path.to_string_lossy().to_string(),
      installed_updates: vec![],
      is_local: true,
    })
  }

  async fn set_release_visibility(&self, release_name: &str, visibility: bool) -> Result<()> {
    let api = self.api_client.current_provider()?;

    api.set_release_visibility(release_name, visibility).await?;

    Ok(())
  }
}
