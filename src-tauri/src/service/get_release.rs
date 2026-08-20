use std::{fs, path::Path};

use crate::{
  configs::AppConfig::Version,
  consts::*,
  handlers::dto::ReleaseManifest,
  providers::dto::{Release, ReleaseAssetGit, ReleaseGit, ReleasePlatform, TreeItem},
  service::{index::ReleaseIndexEntry, main::Service},
  utils::{encoding::read_cp1251_file, patch_markers::read_installed_patches, resources::game_exe},
};

use anyhow::{Result, anyhow, bail};
use futures_util::future::join_all;

/// Identifies the primary ("main_1") repository of a release.
/// Works for both providers: Gitlab names repos "main_1" (bare),
/// while Github names them "<prefix>_main_1". Both forms match here.
fn is_main_repo(name: &str) -> bool {
  name.starts_with("main_1") || name.ends_with("main_1")
}

fn get_platform_from_name(name: &str) -> ReleasePlatform {
  if name == EXE_WIN_NAME {
    ReleasePlatform::Windows
  } else if name == EXE_LINUX_NAME {
    ReleasePlatform::Linux
  } else {
    ReleasePlatform::MacOS
  }
}

/// Build a lightweight ReleaseManifest from index entry fields (no network).
/// Always returns `Some` — even when size fields are zero (old index format).
/// This prevents an infinite spinner in the UI when the fallback manifest
/// fetch also fails (e.g. rate limit).
/// Build a ReleaseManifest from index entry fields (no network).
/// Always returns `Some` — even when size fields are zero (old index format).
/// This prevents an infinite spinner in the UI when the fallback manifest
/// fetch also fails (e.g. rate limit).
/// The per-file list comes from the index assets: without it the manifest is
/// a stub (aggregate sizes only) and the download queue rows show
/// "0 B / 0 B" for every file that is not currently being downloaded.
fn manifest_from_index_entry(entry: &ReleaseIndexEntry) -> Option<ReleaseManifest> {
  let files: Vec<crate::handlers::dto::ReleaseManifestFile> = entry
    .assets
    .iter()
    .map(|a| crate::handlers::dto::ReleaseManifestFile { name: a.name.clone(), size: a.size })
    .collect();

  Some(ReleaseManifest {
    total_files_count: entry.total_files_count,
    total_size: entry.total_size,
    compressed_size: entry.compressed_size,
    files,
    exe_path: entry.exe_path.clone(),
    ..ReleaseManifest::default()
  })
}

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
    let provider_id = api.id();

    // 1. In-memory cache (provider-specific, keyed by provider id).
    if cashed {
      if let Some(cash) = self.releases.get(provider_id) {
        log::info!("get_releases: in-memory cache hit for '{}'", provider_id);
        return Ok(cash.iter().map(|release| Version {
          id: release.id,
          name: release.name.clone(),
          path: release.path.clone(),
          manifest: None,
          engine_path: None,
          fsgame_path: None,
          userltx_path: None,
          exe_path: None,
          installed_path: "".to_owned(),
          download_path: "".to_owned(),
          installed_updates: vec![],
          is_local: false,
        }).collect());
      }
    }

    // 2. Static release index (0 API calls) — works for both cashed=false
    //    (fresh fetch) and cashed=true with empty in-memory cache (e.g.
    //    right after switching providers in the UI).
    match crate::service::index::load_index(provider_id).await {
      Ok(index) => {
        log::info!("get_releases: loaded from static index ({} releases)", index.releases.len());
        let versions: Vec<Version> = index
          .releases
          .iter()
          .enumerate()
          .map(|(i, entry)| Version {
            id: (i + 1) as u32,
            name: entry.name.clone(),
            path: entry.path.clone(),
            manifest: manifest_from_index_entry(entry),
            engine_path: None,
            fsgame_path: None,
            userltx_path: None,
            exe_path: entry.exe_path.clone(),
            installed_path: "".to_owned(),
            download_path: "".to_owned(),
            installed_updates: vec![],
            is_local: false,
          })
          .collect();
        let releases: Vec<Release> = versions
          .iter()
          .map(|v| Release { id: v.id, name: v.name.clone(), path: v.path.clone() })
          .collect();
        self.releases.insert(String::from(provider_id), releases);

        // NOTE: do not warm up the provider's projects_map here. The patch
        // checks (check_patches_available / get_version_patches_impl) are
        // already index-first and only hit the API when the index is down —
        // in which case the fallback path resolves the map itself. A warmup
        // call here would burn the anonymous GitHub rate limit on every
        // launch for no benefit.

        return Ok(versions);
      }
      Err(e) => {
        log::warn!("get_releases: static index unavailable, falling back to API: {}", e);
      }
    }

    // 3. Fallback: live API.
    let releases = api.get_releases(cashed).await?;
    self.releases.insert(String::from(provider_id), releases.clone());

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
        exe_path: None,
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

    // Try the static index first.
    if let Ok(index) = crate::service::index::load_index(api.id()).await {
      if let Some(entry) = index.releases.iter().find(|r| r.path == release_name || r.name == release_name) {
        // Fast path: sizes already embedded in the index (no network needed).
        if let Some(m) = manifest_from_index_entry(entry) {
          log::info!("get_release_manifest '{}': serving from index fields (0 requests)", release_name);
          return Ok(m);
        }
        // Slow path: fetch the manifest via its raw URL.
        log::info!("get_release_manifest '{}': fetching from index manifest URL", release_name);
        let cached = crate::utils::http_cache::fetch(
            &crate::utils::http_cache::SHARED_CLIENT,
            &entry.manifest,
            std::time::Duration::from_secs(crate::consts::CACHE_TTL_RAW_FILE_SECS),
        )
        .await?;
        let manifest: ReleaseManifest = serde_json::from_slice(&cached.bytes)?;
        return Ok(manifest);
      }
    }

    // Fallback: original API path.
    let repos = api.get_release_repos_by_name(release_name.clone()).await?;

    let project = repos
      .iter()
      .find(|r| is_main_repo(&r.name))
      .ok_or_else(|| anyhow!("Repo main_1 not found for release: {}", &release_name))?;

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

    // Try the static release index first.
    if let Ok(index) = crate::service::index::load_index(api.id()).await {
      if let Some(entry) = index.releases.iter().find(|r| r.path == release_name || r.name == release_name) {
        log::info!("get_main_release '{}': loaded from static index", release_name);
        let assets: Vec<ReleaseAssetGit> = entry
          .assets
          .iter()
          .map(|a| ReleaseAssetGit {
            name: a.name.clone(),
            platform: get_platform_from_name(&a.name),
            size: a.size,
            download_link: a.url.clone(),
          })
          .collect();
        return Ok(ReleaseGit {
          name: entry.name.clone(),
          version: entry.tag.clone(),
          assets,
        });
      }
    }

    // Fallback: original API path.

    let repos = api.get_release_repos_by_name(release_name).await?;

    if repos.is_empty() {
      bail!("No 'main_' repos found for release {}", release_name);
    }

    let main_repo = repos
      .iter()
      .find(|r| is_main_repo(&r.name))
      .ok_or_else(|| {
        let names: Vec<&str> = repos.iter().map(|r| r.name.as_str()).collect();
        log::error!(
          "main_1 repo not found for release '{}'. Available repos: {:?}",
          &release_name,
          names
        );
        anyhow!("Repo main_1 not found for release: {}", &release_name)
      })?;

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

      // A non-UTF-8 folder name must not panic the whole background init:
      // launcher-created installs always have UTF-8 names, so skip odd ones.
      let key_path = match entry.file_name().into_string() {
        Ok(name) => name,
        Err(os_name) => {
          log::warn!("Skipping installed version with non-UTF-8 folder name: {:?}", os_name.to_string_lossy());
          continue;
        }
      };
      let name = crate::utils::parse_strings::DASHES_RE.replace_all(&key_path, " ").to_string();

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

      let installed_path_str = path.to_string_lossy().to_string();

      versions.push(Version {
        id: 0,
        name: name,
        path: key_path,
        manifest: None,
        engine_path: None,
        fsgame_path: None,
        userltx_path: None,
        exe_path: None,
        installed_updates: read_installed_patches(&path),
        installed_path: installed_path_str.clone(),
        download_path: installed_path_str,
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
      exe_path: None,
      installed_updates: read_installed_patches(&current_path),
      installed_path: current_path.to_string_lossy().to_string(),
      download_path: current_path.to_string_lossy().to_string(),
      is_local: true,
    })
  }

  async fn set_release_visibility(&self, release_name: &str, visibility: bool) -> Result<()> {
    let api = self.api_client.current_provider()?;

    api.set_release_visibility(release_name, visibility).await?;

    Ok(())
  }
}
