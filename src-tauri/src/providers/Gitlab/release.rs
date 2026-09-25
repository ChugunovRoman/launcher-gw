use std::time::Duration;

use crate::{
  consts::CACHE_TTL_RELEASE_SECS,
  providers::{
    ApiProvider::ApiProvider,
    Gitlab::{Gitlab::Gitlab, group::__update_group, models::*, repo::__update_repo},
    dto::*,
  },
  utils::http_cache,
};

use anyhow::{Context, Result, bail};

/// Lists all releases of a concrete repo (tag, name, body/notes, created_at).
/// Paginated (per_page=100) — default GitLab returns only 20.
pub async fn __get_repo_releases(s: &Gitlab, project_id: &str) -> Result<Vec<RepoReleaseInfo>> {
  const PER_PAGE: usize = 100;
  const MAX_PAGES: u32 = 100;
  let mut all_releases: Vec<ReleaseGitlab> = Vec::new();
  let mut page: u32 = 1;

  loop {
    if page > MAX_PAGES {
      log::warn!("__get_repo_releases: hit MAX_PAGES limit ({})", MAX_PAGES);
      break;
    }
    let url = format!("{}/projects/{}/releases?per_page={}&page={}", &s.host, &project_id, PER_PAGE, page);
    let cached = s.get_cached(&url, Duration::from_secs(CACHE_TTL_RELEASE_SECS)).await?;
    let releases: Vec<ReleaseGitlab> = serde_json::from_slice(&cached.bytes)
      .context("Failed to parse GitLab releases response as JSON")?;
    let len = releases.len();
    all_releases.extend(releases);
    if len < PER_PAGE {
      break;
    }
    page += 1;
  }

  log::info!("GitLab __get_repo_releases: {} releases for project '{}'", all_releases.len(), project_id);

  Ok(all_releases
    .into_iter()
    .map(|r| RepoReleaseInfo {
      tag_name: r.tag_name,
      name: r.name,
      body: r.description,
      created_at: r.created_at,
      // GitLab's created_at is already the release creation date.
      published_at: None,
      assets: r.assets.links.into_iter().map(|a| RepoReleaseAsset {
        name: a.name,
        size: None, // GitLab does not expose size in link objects.
        download_link: a.direct_asset_url,
      }).collect(),
    })
    .collect())
}

pub async fn __get_releases(s: &Gitlab, cashed: bool) -> Result<Vec<Release>> {
  let root_id = s.get_manifest()?.root_id.context("Cannot get root_id from Gitlab manifest file!")?;

  const PER_PAGE: usize = 100;
  const MAX_PAGES: u32 = 100; // safety guard
  let mut all_groups: Vec<Group> = Vec::new();
  let mut page: u32 = 1;

  loop {
    if page > MAX_PAGES {
      log::warn!("__get_releases: hit MAX_PAGES limit ({})", MAX_PAGES);
      break;
    }
    let url = format!("{}/groups/{}/subgroups?sort=desc&per_page={}&page={}", &s.host, &root_id, PER_PAGE, page);
    let resp = s.get(&url).send().await.context("Failed to send request to GitLab (get_releases)")?;

    if !resp.status().is_success() {
      let status = resp.status();
      let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
      bail!("__get_releases, GitLab API error {}: {} url: {}", status, body, url);
    }

    let groups: Vec<Group> = resp.json().await.context("Failed to parse GitLab groups response as JSON")?;
    let len = groups.len();
    all_groups.extend(groups);
    // REGR-2: exit when page is not full (avoids one extra empty request).
    if len < PER_PAGE {
      break;
    }
    page += 1;
  }

  let versions = all_groups
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
    .find(|r| r.name == release_name || r.path == release_name)
    .ok_or_else(|| anyhow::anyhow!(
      "get_release_repos_by_name(): release '{}' not found (checked name and path)", release_name
    ))?;

  let repos = __get_release_repos(s, &release.id.to_string()).await?;

  Ok(repos)
}

/// Fetch all projects of a GitLab group with pagination (per_page=100).
async fn __get_group_projects_paginated(s: &Gitlab, group_id: &str) -> Result<Vec<ProjectGitlab>> {
  const PER_PAGE: usize = 100;
  const MAX_PAGES: u32 = 100;
  let mut all_repos: Vec<ProjectGitlab> = Vec::new();
  let mut page: u32 = 1;

  loop {
    if page > MAX_PAGES {
      log::warn!("__get_group_projects_paginated: hit MAX_PAGES limit ({})", MAX_PAGES);
      break;
    }
    let url = format!("{}/groups/{}/projects?per_page={}&page={}", &s.host, group_id, PER_PAGE, page);
    let resp = s.get(&url).send().await.context("Failed to send request to GitLab (get_group_projects)")?;

    if !resp.status().is_success() {
      let status = resp.status();
      let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
      bail!("__get_group_projects_paginated, GitLab API error {}: {} url: {}", status, body, url);
    }

    let repos: Vec<ProjectGitlab> = resp.json().await.context("Failed to parse GitLab projects response as JSON")?;
    let len = repos.len();
    all_repos.extend(repos);
    if len < PER_PAGE {
      break;
    }
    page += 1;
  }

  Ok(all_repos)
}

async fn __get_release_repos(s: &Gitlab, release_id: &str) -> Result<Vec<Project>> {
  let repos = __get_group_projects_paginated(s, release_id).await?;

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
  let repos = __get_group_projects_paginated(s, release_id).await?;

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

  // Paginated fetch of all projects in the release group.
  let repos = __get_group_projects_paginated(s, &release_id.to_string()).await?;

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

pub async fn __create_tag(s: &Gitlab, repo_id: &str, tag_name: &str, branch: &str) -> Result<()> {
  let url = format!("{}/projects/{}/repository/tags?tag_name={}&ref={}", s.host, repo_id, tag_name, branch);
  let resp = s.post(&url).send().await.context("Failed to send request to Gitlab (__create_tag)")?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("__create_tag, Gitlab API error {}: {} url: {}", status, body, url);
  }

  Ok(())
}

pub async fn __create_release(s: &Gitlab, repo_id: &str, tag_name: &str, assets: Vec<CreateReleaseAsset>) -> Result<CreateReleaseResponse> {
  let url = format!("{}/projects/{}/releases", s.host, repo_id);
  let body = CreateReleaseRequestGitlab {
    name: format!("Release {}", &tag_name),
    tag_name: tag_name.to_string(),
    description: format!("Release {}", &tag_name),
    assets: CreateReleaseAssetsGitlab {
      links: assets
        .iter()
        .map(|asset| CreateReleaseAssetGitlab {
          name: asset.file_name.clone(),
          url: asset.file_download_url.clone(),
        })
        .collect(),
    },
  };

  let resp = s
    .post(&url)
    .json(&body)
    .send()
    .await
    .context("Failed to send request to Gitlab (__create_release)")?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("__create_release, GitLab API error {}: {} url: {}", status, body, url);
  }

  let _: CreateReleaseResponseGitlab = resp.json().await?;

  Ok(CreateReleaseResponse {
    id: 0,
    upload_url: format!("{}/projects/<PROJECT_ID>/packages/generic/<NAME_SPACE>/<VERSION>/<FILE_NAME>", s.host),
  })
}

/// SHA-256 hashes of a tag's assets from the Generic Packages API: the
/// release assets are links to `packages/generic/<ns>/<tag>/<file>` files.
/// A previous failed delete + re-upload cycle can leave more than one
/// `package_file` row with the same name — keep only the newest (highest id)
/// per name so a stale duplicate is never reported as the current asset.
pub async fn __get_release_assets_sha256(s: &Gitlab, project_id: &str, tag_name: &str) -> Result<Vec<AssetSha256>> {
  let files = gitlab_package_files(s, project_id, tag_name).await?;
  let mut by_name: std::collections::HashMap<String, GitlabPackageFile> = std::collections::HashMap::new();
  for f in files {
    by_name.entry(f.file_name.clone()).and_modify(|existing| if f.id > existing.id { *existing = f.clone() }).or_insert(f);
  }
  Ok(
    by_name
      .into_values()
      .map(|f| AssetSha256 {
        name: f.file_name,
        size: f.size,
        sha256: f.file_sha256,
      })
      .collect(),
  )
}

/// Delete an asset's package file(s) (re-upload path after a hash mismatch).
/// Deletes EVERY `package_file` row with this name, not just the first match:
/// a previous failed delete can leave a stale duplicate behind that a
/// single-row delete would miss, causing the next verify to keep comparing
/// against the old (mismatching) row forever.
pub async fn __delete_release_asset(s: &Gitlab, project_id: &str, tag_name: &str, file_name: &str) -> Result<()> {
  let files = gitlab_package_files(s, project_id, tag_name).await?;
  let matches: Vec<&GitlabPackageFile> = files.iter().filter(|f| f.file_name == file_name).collect();
  if matches.is_empty() {
    bail!("Package file '{}' not found for tag '{}'", file_name, tag_name);
  }

  for file in matches {
    let url = format!("{}/projects/{}/packages/{}/package_files/{}", s.host, project_id, file.package_id, file.id);
    let resp = s
      .delete(&url)
      .send()
      .await
      .context("Failed to send request to Gitlab (__delete_release_asset)")?;

    if !resp.status().is_success() && resp.status().as_u16() != 404 {
      let status = resp.status();
      let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
      bail!("__delete_release_asset, GitLab API error {}: {} url: {}", status, body, url);
    }
  }
  Ok(())
}

/// Find the generic package of a tag and list its files, paginated (GitLab
/// caps unpaginated listings at 20 rows).
async fn gitlab_package_files(s: &Gitlab, project_id: &str, tag_name: &str) -> Result<Vec<GitlabPackageFile>> {
  let namespace = crate::consts::GENERIC_PACKAGE_NAMESPACE;

  // `package_name`/`package_version` are ILIKE (substring) filters on
  // GitLab's side, so match exactly on name/version/type instead of trusting
  // the first hit — and paginate, since a long-lived project can accumulate
  // many stale generic packages under the same name.
  let mut packages: Vec<GitlabPackage> = Vec::new();
  let mut page: u32 = 1;
  loop {
    let url = format!(
      "{}/projects/{}/packages?package_name={}&package_version={}&page={}&per_page=100",
      s.host, project_id, namespace, tag_name, page
    );
    let resp = s
      .get(&url)
      .send()
      .await
      .context("Failed to send request to Gitlab (packages lookup)")?;

    if !resp.status().is_success() {
      let status = resp.status();
      let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
      bail!("gitlab_package_files, GitLab API error {}: {} url: {}", status, body, url);
    }

    let page_items: Vec<GitlabPackage> = resp.json().await.context("Failed to parse GitLab packages response")?;
    if page_items.is_empty() {
      break;
    }
    packages.extend(page_items);
    page += 1;
  }

  let package = packages
    .into_iter()
    .filter(|p| p.name == namespace && p.version == tag_name && p.package_type.as_deref() == Some("generic"))
    // Same reasoning as the package_files dedup above: pick the live (newest) one.
    .max_by_key(|p| p.id)
    .ok_or_else(|| anyhow::anyhow!("Generic package '{}'/'{}' not found in project {}", namespace, tag_name, project_id))?;

  let mut files: Vec<GitlabPackageFile> = Vec::new();
  let mut page: u32 = 1;
  loop {
    let url = format!("{}/projects/{}/packages/{}/package_files?page={}&per_page=100", s.host, project_id, package.id, page);
    let resp = s
      .get(&url)
      .send()
      .await
      .context("Failed to send request to Gitlab (package_files lookup)")?;

    if !resp.status().is_success() {
      let status = resp.status();
      let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
      bail!("gitlab_package_files, GitLab API error {}: {} url: {}", status, body, url);
    }

    let page_items: Vec<GitlabPackageFile> = resp.json().await.context("Failed to parse GitLab package_files response")?;
    if page_items.is_empty() {
      break;
    }
    files.extend(page_items);
    page += 1;
  }

  Ok(files)
}
