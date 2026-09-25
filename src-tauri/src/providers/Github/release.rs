use std::{collections::HashMap, vec};

use crate::{
  consts::{GITHUB_ORG, INDEX_REPO_NAME, CACHE_TTL_ORG_REPOS_SECS, CACHE_TTL_RELEASE_SECS},
  providers::{
    ApiProvider::ApiProvider,
    Github::{Github::Github, models::*, repo::*},
    dto::*,
  },
};

use anyhow::{Context, Result, bail};
use std::time::Duration;

use crate::consts::JSON_ERROR_BODY_PREVIEW_LEN;

/// First bytes of a response body, lossily decoded and truncated, for error
/// messages. Keeps a non-JSON answer (HTML login page, rate-limit notice)
/// visible in the log instead of a bare serde position error.
fn body_preview(bytes: &[u8]) -> String {
  let end = JSON_ERROR_BODY_PREVIEW_LEN.min(bytes.len());
  String::from_utf8_lossy(&bytes[..end]).replace(['\n', '\r'], " ")
}

async fn __fetch_releases(s: &Github, cashed: bool) -> Result<()> {
  const PER_PAGE: u32 = 100;
  let mut map: HashMap<u32, ProjectGithub> = HashMap::new();
  let mut page: u32 = 1;
  let mut release_count = crate::utils::locks::lock(&s.projects_map).len();

  if !cashed {
    release_count = 0;
  }

  loop {
    if release_count > 0 {
      break;
    }

    let url = format!("{}/orgs/{}/repos?per_page={}&page={}", s.host, GITHUB_ORG, PER_PAGE, page);

    // `cashed == false` is the explicit "Refresh" path. Resetting `release_count`
    // alone was not enough: `get_cached` with the 1-hour TTL answered straight
    // from disk without touching the network, so a newly published repo did not
    // appear in the list for up to an hour. `fetch_force` always revalidates —
    // the ETag usually yields 304, so the traffic cost is negligible.
    let cached = if cashed {
      crate::utils::http_cache::fetch(&s.get_client(), &url, Duration::from_secs(CACHE_TTL_ORG_REPOS_SECS)).await
    } else {
      crate::utils::http_cache::fetch_force(&s.get_client(), &url).await
    }
    .context("Failed to send request to Github (__get_releases)")?;

    let projects: Vec<ProjectGithub> = serde_json::from_slice(&cached.bytes).map_err(|e| {
      // A raw serde error ("expected value at line 1") hides what actually came
      // back — a GitHub error object, a rate-limit body or a captive-portal HTML
      // page (served with 200, so the cache stored it happily).
      anyhow::anyhow!(
        "Failed to parse Github repos response as JSON ({}): url={}, source={:?}, body[..{}]={}",
        e,
        url,
        cached.source,
        JSON_ERROR_BODY_PREVIEW_LEN.min(cached.bytes.len()),
        body_preview(&cached.bytes)
      )
    })?;

    let len = projects.len();

    if projects.is_empty() {
      // `break`, NOT `return`: returning here skips the assignment below, so an
      // org whose repo count is an exact multiple of per_page (page N full,
      // page N+1 empty) ends up with an EMPTY projects_map and therefore an
      // empty release list, re-paginating the API on every subsequent call.
      log::info!("Github __get_releases: page {} empty, stopping pagination", page);
      break;
    }

    for project in projects {
      map.insert(project.id, project);
    }

    if (len as u32) < PER_PAGE {
      break;
    }

    page += 1;
  }

  if release_count == 0 {
    *crate::utils::locks::lock(&s.projects_map) = map;
  }

  Ok(())
}

pub async fn __get_releases(s: &Github, cashed: bool) -> Result<Vec<Release>> {
  __fetch_releases(s, cashed).await?;

  let cached_projects = crate::utils::locks::lock(&s.projects_map).clone();
  let mut releases: Vec<Release> = vec![];
  let mut exist_names: HashMap<String, bool> = HashMap::new();

  for (id, project) in cached_projects {
    // Infrastructure repo of the static release index — never a game release.
    if project.name == INDEX_REPO_NAME {
      continue;
    }

    let desc = project.description.clone().unwrap_or_default();
    // C6: fallback to repo name (strip _main_N / _updates_N suffix) when the
    // description is empty, instead of silently losing the release.  Only a
    // repo that actually carries the release suffix can yield a release name —
    // anything else is an infrastructure repo and must not become a phantom
    // entry in the versions list.
    let release_name = if desc.is_empty() {
      let Some(stripped) = project
        .name
        .rsplit_once("_main_")
        .or_else(|| project.name.rsplit_once("_updates_"))
        .map(|(prefix, _)| prefix.to_string())
      else {
        log::debug!("GitHub: skipping non-release repo '{}' (no description, no release suffix)", project.name);
        continue;
      };
      log::warn!(
        "GitHub: repo '{}' has no description, using name-derived release name '{}'",
        project.name, stripped
      );
      stripped
    } else {
      desc
    };
    if let None = exist_names.get(&release_name) {
      releases.push(Release {
        id,
        name: release_name.clone(),
        path: crate::utils::parse_strings::WHITESPACE_RE.replace_all(&release_name, "-").to_string(),
      });
      exist_names.insert(release_name, true);
    }
  }

  Ok(releases)
}

pub async fn __get_release_repos_by_name(s: &Github, release_name: &str) -> Result<Vec<Project>> {
  __fetch_releases(s, true).await?;

  let cached_projects = crate::utils::locks::lock(&s.projects_map).clone();
  let mut repos: Vec<Project> = vec![];

  for (id, project) in cached_projects {
    if let Some(pos) = project.name.find("_main_")
      && pos > 0
    {
      // Match by description (normal case) or by derived name (repo without description).
      let matches = project.description.as_deref() == Some(release_name)
        || (project.description.as_deref().unwrap_or("").is_empty() && {
          let derived = project.name[..project.name.rfind("_main_").unwrap()].to_string();
          derived == release_name
        });
      if matches {
        repos.push(Project {
          id,
          name: project.name.clone(),
          path: project.name,
          ssh_remote_url: project.ssh_url,
          marked_for_deletion_on: None,
        });
      }
    }
  }

  Ok(repos)
}

/// Lists all releases of a concrete repo (tag, name, body/notes, created_at).
/// Paginated (per_page=100) — default GitHub returns only 30.
pub async fn __get_repo_releases(s: &Github, project_id: &str) -> Result<Vec<RepoReleaseInfo>> {
  const PER_PAGE: usize = 100;
  const MAX_PAGES: u32 = 100;
  let mut all_releases: Vec<ReleaseGithub> = Vec::new();
  let mut page: u32 = 1;

  loop {
    if page > MAX_PAGES {
      log::warn!("__get_repo_releases: hit MAX_PAGES limit ({})", MAX_PAGES);
      break;
    }
    let url = format!("{}/repos/{}/{}/releases?per_page={}&page={}", &s.host, GITHUB_ORG, &project_id, PER_PAGE, page);
    let cached = s.get_cached(&url, Duration::from_secs(CACHE_TTL_RELEASE_SECS)).await?;
    let releases: Vec<ReleaseGithub> = serde_json::from_slice(&cached.bytes)
      .context("Failed to parse Github releases response as JSON")?;
    let len = releases.len();
    all_releases.extend(releases);
    if len < PER_PAGE {
      break;
    }
    page += 1;
  }

  log::info!("Github __get_repo_releases: {} releases for project '{}'", all_releases.len(), project_id);

  Ok(all_releases
    .into_iter()
    .map(|r| RepoReleaseInfo {
      tag_name: r.tag_name,
      name: r.name,
      body: r.body,
      created_at: r.created_at,
      published_at: r.published_at,
      assets: r.assets.into_iter().map(|a| RepoReleaseAsset {
        name: a.name,
        size: Some(a.size),
        download_link: a.browser_download_url,
      }).collect(),
    })
    .collect())
}

pub async fn __get_updates_repos_by_name(s: &Github, release_name: &str) -> Result<Vec<Project>> {
  __fetch_releases(s, true).await?;

  let cached_projects = crate::utils::locks::lock(&s.projects_map).clone();
  let mut repos: Vec<Project> = vec![];

  for (id, project) in cached_projects {
    if let Some(pos) = project.name.find("_updates_")
      && pos > 0
    {
      let matches = project.description.as_deref() == Some(release_name)
        || (project.description.as_deref().unwrap_or("").is_empty() && {
          let derived = project.name[..project.name.rfind("_updates_").unwrap()].to_string();
          derived == release_name
        });
      if matches {
        repos.push(Project {
          id,
          name: project.name.clone(),
          path: project.name,
          ssh_remote_url: project.ssh_url,
          marked_for_deletion_on: None,
        });
      }
    }
  }

  Ok(repos)
}

pub async fn __set_release_visibility(s: &Github, release_name: &str, visibility: bool) -> Result<()> {
  __fetch_releases(s, true).await?;

  let cached_projects = crate::utils::locks::lock(&s.projects_map).clone();

  for (_, project) in cached_projects {
    let matches = project.description.as_deref() == Some(release_name)
      || (project.description.as_deref().unwrap_or("").is_empty()
        && project
          .name
          .rsplit_once("_main_")
          .or_else(|| project.name.rsplit_once("_updates_"))
          // Only repos that actually carry the release suffix may be matched
          // by a derived name — otherwise ANY description-less repo whose name
          // equals the release name would have its visibility flipped.
          .is_some_and(|(prefix, _)| prefix == release_name));
    if matches {
      __update_repo(
        s,
        &project.name,
        UpdateRepoDtoGithub {
          visibility: if visibility {
            Some("public".to_owned())
          } else {
            Some("private".to_owned())
          },
          private: Some(!visibility),
          ..UpdateRepoDtoGithub::default()
        },
      )
      .await?;
    }
  }

  Ok(())
}

pub async fn __create_release(s: &Github, repo_id: &str, tag_name: &str, assets: Vec<CreateReleaseAsset>) -> Result<CreateReleaseResponse> {
  let url = format!("{}/repos/{}/{}/releases", s.host, GITHUB_ORG, repo_id);
  let body = CreateReleaseRequestGithub {
    name: format!("Release {}", &tag_name),
    tag_name: tag_name.to_string(),
    target_commitish: "master".to_string(),
  };

  let resp = s
    .post(&url)
    .json(&body)
    .send()
    .await
    .context("Failed to send request to Github (__create_release)")?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("__create_release, Github API error {}: {} url: {}", status, body, url);
  }

  let response: CreateReleaseResponseGithub = resp.json().await?;
  let mut upload_url = response.upload_url.clone();

  if let Some((base, _)) = upload_url.split_once('{') {
    upload_url = base.to_string();
  }

  upload_url = format!("{}?name=<FILE_NAME>&label=<FILE_NAME>", upload_url);

  Ok(CreateReleaseResponse { id: response.id, upload_url })
}

/// SHA-256 hashes of a tag's assets straight from the release metadata
/// (`digest` field). Deliberately NOT cached — callers need a fresh answer
/// right after an upload.
pub async fn __get_release_assets_sha256(s: &Github, project_id: &str, tag_name: &str) -> Result<Vec<AssetSha256>> {
  let url = format!("{}/repos/{}/{}/releases/tags/{}", s.host, GITHUB_ORG, project_id, tag_name);
  let resp = s
    .get(&url)
    .send()
    .await
    .context("Failed to send request to Github (__get_release_assets_sha256)")?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("__get_release_assets_sha256, Github API error {}: {} url: {}", status, body, url);
  }

  let release: ReleaseGithub = resp.json().await.context("Failed to parse Github release-by-tag response")?;

  Ok(release
    .assets
    .into_iter()
    .map(|a| AssetSha256 {
      name: a.name,
      size: Some(a.size),
      // GitHub reports "sha256:<hex>"; strip the algorithm prefix.
      sha256: a.digest.map(|d| d.strip_prefix("sha256:").unwrap_or(&d).to_string()),
    })
    .collect())
}

/// Delete an asset from a release (re-upload path after a hash mismatch).
pub async fn __delete_release_asset(s: &Github, project_id: &str, tag_name: &str, file_name: &str) -> Result<()> {
  let assets = __get_release_assets_sha256_detail(s, project_id, tag_name).await?;
  let asset = assets
    .into_iter()
    .find(|a| a.name == file_name)
    .ok_or_else(|| anyhow::anyhow!("Asset '{}' not found in release '{}'", file_name, tag_name))?;

  let url = format!("{}/repos/{}/{}/releases/assets/{}", s.host, GITHUB_ORG, project_id, asset.id);
  let resp = s
    .delete(&url)
    .send()
    .await
    .context("Failed to send request to Github (__delete_release_asset)")?;

  if !resp.status().is_success() && resp.status().as_u16() != 404 {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("__delete_release_asset, Github API error {}: {} url: {}", status, body, url);
  }
  Ok(())
}

/// Same as __get_release_assets_sha256 but keeps the raw asset records
/// (ids included) for internal use (asset deletion).
async fn __get_release_assets_sha256_detail(s: &Github, project_id: &str, tag_name: &str) -> Result<Vec<ReleaseAssetGithub>> {
  let url = format!("{}/repos/{}/{}/releases/tags/{}", s.host, GITHUB_ORG, project_id, tag_name);
  let resp = s
    .get(&url)
    .send()
    .await
    .context("Failed to send request to Github (__get_release_assets_sha256_detail)")?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("__get_release_assets_sha256_detail, Github API error {}: {} url: {}", status, body, url);
  }

  let release: ReleaseGithub = resp.json().await.context("Failed to parse Github release-by-tag response")?;
  Ok(release.assets)
}
