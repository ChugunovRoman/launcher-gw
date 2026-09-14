use std::{fs, path::Path, time::Instant};

use crate::{
  configs::AppConfig::{AppConfig, Version},
  consts::*,
  handlers::dto::ReleaseManifest,
  providers::dto::{ReleaseAssetGit, ReleaseGit, ReleasePlatform, TreeItem},
  service::{index::ReleaseIndexEntry, main::Service},
  utils::{encoding::read_cp1251_file, patch_markers::read_installed_patches, resources::game_exe},
};

use anyhow::{Result, anyhow, bail};
use futures_util::future::join_all;

/// Controls how `get_releases` resolves the version list.
///
/// - `Cached` — in-memory (with TTL) → index → API.  Normal UI updates.
/// - `IndexFirst` — index → API, skips in-memory.  Launcher startup, players.
/// - `ApiOnly` — API only, index ignored.  Dev operations (upload, patch).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseSource {
  Cached,
  IndexFirst,
  ApiOnly,
}

/// TTL for the in-memory releases cache (60 seconds).
/// During burst reads (e.g. multiple UI requests in one second) the cache
/// prevents hammering the index/API; after expiry the next request refreshes.
const RELEASES_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(60);

/// Identifies the primary ("main_1") repository of a release.
/// Works for both providers: Gitlab names repos "main_1" (bare),
/// while Github names them "<prefix>_main_1". Both forms match here.
pub(crate) fn is_main_repo(name: &str) -> bool {
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
    .map(|a| crate::handlers::dto::ReleaseManifestFile {
      name: a.name.clone(),
      size: a.size,
      sha256: a.sha256.clone(),
      kind: a.kind,
      target: a.target.clone(),
    })
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

/// Infrastructure repositories that must never be shown as a game version.
/// The static-index repo is a normal repo in the same org, so it can reach the
/// release list through the API path; a previously published index may also
/// still contain it.  Filtered here so BOTH views (Releases and Versions) and
/// every consumer of the list see the same clean set.
fn is_infrastructure_release(name: &str, path: &str) -> bool {
  name.eq_ignore_ascii_case(INDEX_REPO_NAME) || path.eq_ignore_ascii_case(INDEX_REPO_NAME)
}

pub trait ServiceGetRelease {
  async fn get_releases(&mut self, source: ReleaseSource) -> Result<Vec<Version>>;
  async fn refresh_releases(&mut self) -> Result<Vec<Version>>;
  async fn get_release_manifest(&self, release_name: &str) -> Result<ReleaseManifest>;
  async fn get_main_release_files(&self, release_id: &str) -> Result<Vec<TreeItem>>;
  async fn get_main_release(&self, release_name: &str) -> Result<ReleaseGit>;
  async fn set_release_visibility(&self, path: &str, visibility: bool) -> Result<()>;
}

impl ServiceGetRelease for Service {
  async fn get_releases(&mut self, source: ReleaseSource) -> Result<Vec<Version>> {
    let api = self.api_client.current_provider()?;
    let provider_id = api.id().to_string();

    // 1. In-memory cache (provider-specific, keyed by provider id, with TTL).
    if source == ReleaseSource::Cached {
      if let Some((instant, versions)) = self.releases_cache.get(&provider_id) {
        if instant.elapsed() < RELEASES_CACHE_TTL {
          log::info!("get_releases: in-memory cache hit for '{}' (TTL active)", provider_id);
          return Ok(versions.clone());
        } else {
          log::info!("get_releases: in-memory cache expired for '{}', refreshing", provider_id);
        }
      }
    }

    // 2. Static release index (0 API calls) — used by Cached (after cache miss)
    //    and IndexFirst (startup).  ApiOnly skips the index entirely.
    if source != ReleaseSource::ApiOnly {
      match crate::service::index::load_index(&provider_id).await {
        Ok(index) => {
          log::info!("get_releases: loaded from static index ({} releases)", index.releases.len());
          let versions: Vec<Version> = index
            .releases
            .iter()
            .filter(|entry| !is_infrastructure_release(&entry.name, &entry.path))
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

          self.releases_cache.insert(provider_id, (Instant::now(), versions.clone()));

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
    }

    // 3. Fallback: live API (used by ApiOnly or when index is unavailable).
    let releases = api.get_releases(source == ReleaseSource::Cached).await?;

    let versions: Vec<Version> = releases
      .iter()
      .filter(|release| !is_infrastructure_release(&release.name, &release.path))
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

    // CRIT-1: Do NOT cache ApiOnly results — they come without manifests
    // and would poison the cache for 60 seconds, causing D1 (0-byte sizes,
    // "Wait" button) for any concurrent UI request.
    if source != ReleaseSource::ApiOnly {
      self.releases_cache.insert(provider_id, (Instant::now(), versions.clone()));
    }

    Ok(versions)
  }

  /// Force-refresh the releases list.  Invalidates the in-memory cache and
  /// re-reads the static index (conditional GET via ETag, cheap for players).
  /// When the provider has a token (dev mode), additionally fetches from the
  /// live API and merges releases not yet present in the index — so dev sees
  /// freshly created releases before `publish_index` runs.
  async fn refresh_releases(&mut self) -> Result<Vec<Version>> {
    // Invalidate so the next get_releases call skips the in-memory cache.
    self.invalidate_releases();

    // Extract provider info before the mutable borrow in get_releases.
    let (has_token, provider_id) = {
      let api = self.api_client.current_provider()?;
      (!api.get_token().is_empty(), api.id().to_string())
    };

    // CRIT-3: Force ETag revalidation (TTL=0) so an index published from
    // another machine is picked up within seconds, not after 10 minutes.
    let mut versions = {
      let api = self.api_client.current_provider()?;
      let provider_id = api.id().to_string();

      match crate::service::index::load_index_with_ttl(&provider_id, std::time::Duration::ZERO).await {
        Ok(index) => {
          log::info!("refresh_releases: loaded from index with forced revalidation ({} releases)", index.releases.len());
          let versions: Vec<Version> = index
            .releases
            .iter()
            .filter(|entry| !is_infrastructure_release(&entry.name, &entry.path))
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
          self.releases_cache.insert(provider_id, (Instant::now(), versions.clone()));
          versions
        }
        Err(e) => {
          // IndexFirst (not Cached): a forced refresh must not reuse the
          // provider's stale projects_map, otherwise a repo created moments
          // ago stays invisible even though the user asked for fresh data.
          log::warn!("refresh_releases: index unavailable, falling back to a fresh API fetch: {}", e);
          self.get_releases(ReleaseSource::IndexFirst).await?
        }
      }
    };

    // Dev mode: merge API-only releases that are not yet in the index.
    if has_token {
      // Collect API releases in a block to limit the immutable borrow scope.
      let api_releases_result = {
        let api = self.api_client.current_provider()?;
        api.get_releases(false).await
      };
      match api_releases_result {
        Ok(api_releases) => {
          // CRIT-4: Normalize names for dedup — index may store "Global War Dev"
          // while repo-derived name (no description) yields "Global-War-Dev".
          let normalize = |s: &str| s.replace('-', " ").to_lowercase();
          let existing: std::collections::HashSet<String> =
            versions.iter().map(|v| normalize(&v.name)).collect();
          let mut added = 0u32;
          for r in api_releases {
            if is_infrastructure_release(&r.name, &r.path) {
              continue;
            }
            if !existing.contains(&normalize(&r.name)) {
              versions.push(Version {
                id: r.id,
                name: r.name.clone(),
                path: r.path.clone(),
                manifest: None,
                engine_path: None,
                fsgame_path: None,
                userltx_path: None,
                exe_path: None,
                installed_path: "".to_owned(),
                download_path: "".to_owned(),
                installed_updates: vec![],
                is_local: false,
              });
              added += 1;
            }
          }
          if added > 0 {
            log::info!("refresh_releases: merged {} API-only releases", added);
            self.releases_cache.insert(provider_id, (Instant::now(), versions.clone()));
          }
        }
        Err(e) => {
          log::warn!("refresh_releases: API fetch failed (non-fatal): {}", e);
        }
      }
    }

    Ok(versions)
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

  async fn set_release_visibility(&self, release_name: &str, visibility: bool) -> Result<()> {
    let api = self.api_client.current_provider()?;

    api.set_release_visibility(release_name, visibility).await?;

    Ok(())
  }
}

/// Standalone version of `get_local_version` that reads directly from
/// `AppConfig` without requiring a `Service` lock.  Pure local I/O, no network.
pub async fn get_local_version_from_config(config: &AppConfig) -> Result<Vec<Version>> {
  let install_path = &config.default_installed_path;
  let progress_download = &config.progress_download;
  let versions_dir = Path::new(install_path);

  let mut versions: Vec<Version> = vec![];

  if !versions_dir.exists() {
    return Ok(versions);
  }

  for entry in std::fs::read_dir(versions_dir)? {
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
      "get_local_version_from_config, name {:?} path: {:?}",
      &name, &path,
    );

    let installed_path_str = path.to_string_lossy().to_string();

    versions.push(Version {
      id: 0,
      name,
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

/// Standalone version of `get_main_version` that reads directly from
/// `AppConfig` without requiring a `Service` lock.  Pure local I/O, no network.
pub async fn get_main_version_from_config(config: &AppConfig) -> Option<Version> {
  let current_path = Path::new(&config.install_path).to_owned();

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
