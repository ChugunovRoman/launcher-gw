// Writer: rebuild the static release index and commit it to the index repo.
//
// Per-provider: the writer publishes an index for the *currently selected*
// provider.  Dev loads a release on GitHub → GitHub index is updated; then
// switches to GitLab, loads the same release → GitLab index is updated.
// Each index contains provider-specific download URLs so players never
// cross providers.
//
// Called after every successful full upload or patch upload (best-effort:
// errors are logged but never abort the finished upload).
// Also exposed as a manual "Re-publish index" button in the Releases view.

use std::{collections::HashMap, time::Duration};

use anyhow::{Context, Result, bail};
use serde_json;

use crate::{
  consts::*,
  handlers::dto::ReleaseManifest,
  providers::{ApiProvider::ApiProvider, dto::Project},
  service::index::*,
};

/// Collect the release index JSON from live API data (no commit).
/// Used for preview in the UI before the dev confirms the commit.
pub async fn collect_index(api: &(dyn ApiProvider + Send + Sync)) -> Result<String> {
  let index = collect_release_index(api).await?;
  let index = merge_existing_dev_managed_fields(api, index).await?;
  serde_json::to_string_pretty(&index).context("index: serialize")
}

/// Commit a previously collected index JSON string to the provider's index repo.
pub async fn commit_index_json(api: &(dyn ApiProvider + Send + Sync), json: &str) -> Result<()> {
  let is_gitlab = api.is_suppot_subgroups();
  if is_gitlab {
    if GITLAB_INDEX_PROJECT_ID == 0 {
      bail!("GITLAB_INDEX_PROJECT_ID = 0, GitLab index not configured");
    }
    api
      .add_file_to_repo(
        &GITLAB_INDEX_PROJECT_ID.to_string(),
        "index.json",
        json,
        "Update release index",
        DEFAULT_BRANCH,
      )
      .await
      .context("index: add_file_to_repo (GitLab)")?;
  } else {
    api
      .add_file_to_repo(INDEX_REPO_NAME, "index.json", json, "Update release index", DEFAULT_BRANCH)
      .await
      .context("index: add_file_to_repo (GitHub)")?;
  }

  // Reflect the just-committed content in the local disk cache immediately.
  // Without this, a `load_index` call within the next INDEX_CACHE_TTL_SECS
  // (e.g. another publish_index right after a manual commit_index) would see
  // the OLD cached body and silently overwrite the fields that were just
  // hand-edited and committed (`presets`, `users`) — the origin's raw-file
  // CDN can also lag a push by a few seconds, so even a forced re-fetch is
  // not reliable immediately after commit. Failure here is non-fatal: the
  // remote commit already succeeded, and the cache will simply revalidate
  // normally after TTL expiry.
  match crate::service::index::index_raw_url(api.id()) {
    Ok(url) => {
      if let Err(e) = crate::utils::http_cache::store(&url, json.as_bytes()) {
        log::warn!("commit_index_json: failed to refresh local cache for '{}': {}", url, e);
      }
    }
    Err(e) => log::warn!("commit_index_json: cannot resolve index URL to refresh cache: {}", e),
  }

  Ok(())
}

/// Collect the release index from live API data (no network commit).
async fn collect_release_index(api: &(dyn ApiProvider + Send + Sync)) -> Result<ReleaseIndex> {
  let is_gitlab = api.is_suppot_subgroups();

  // ---- Launcher (self-update) ----
  let launcher_project_id = if is_gitlab {
    REPO_LAUNCGER_ID_2.to_string()
  } else {
    GITHUB_LAUNCHER_REPO_NAME.to_string()
  };
  let launcher_owner = if is_gitlab { "" } else { MAIN_DEVELOPER_NAME };

  let launcher_release = api
    .get_launcher_latest_release(launcher_owner, &launcher_project_id)
    .await
    .context("index: get_launcher_latest_release")?;

  let bg_etag = fetch_bg_etag(&api.launcher_bg_url()).await;

  let launcher_index = LauncherIndex {
    version: launcher_release.version.clone(),
    assets: launcher_release
      .assets
      .iter()
      .map(|a| IndexLauncherAsset {
        name: a.name.clone(),
        platform: format!("{:?}", a.platform).to_lowercase(),
        size: a.size,
        url: a.download_link.clone(),
      })
      .collect(),
    bg_etag,
  };

  // ---- Game releases ----
  let releases_raw = api.get_releases(false).await.context("index: get_releases")?;

  let mut release_entries: Vec<ReleaseIndexEntry> = Vec::new();

  for release in &releases_raw {
    // Use release.name (original description with spaces) for API lookups,
    // not release.path (where spaces are replaced with dashes).
    let repos = match api.get_release_repos_by_name(&release.name).await {
      Ok(r) => r,
      Err(e) => {
        log::warn!("index: get_release_repos_by_name('{}') failed, skipping: {}", &release.name, e);
        continue;
      }
    };

    let main_repo = repos.iter().find(|r| is_main_repo(&r.name));

    let Some(main) = main_repo else {
      log::warn!("index: no main_1 repo for release '{}', skipping", &release.name);
      continue;
    };

    let project_id = project_id_for_api(api, main);

    let latest = match api
      .get_launcher_latest_release(if is_gitlab { "" } else { GITHUB_ORG }, &project_id)
      .await
    {
      Ok(r) => r,
      Err(e) => {
        log::warn!("index: get_launcher_latest_release('{}') failed, skipping: {}", &project_id, e);
        continue;
      }
    };

    let manifest_url = manifest_url_for(api, main);

    // Extract size fields + exe_path from the release manifest (CDN, not rate-limited).
    let manifest_info = extract_manifest_info(&manifest_url).await;
    if manifest_info.is_none() {
      log::warn!("index: cannot fetch manifest for '{}', sizes will be 0", &release.name);
    }

    let assets: Vec<IndexAsset> = latest
      .assets
      .iter()
      .map(|a| IndexAsset {
        name: a.name.clone(),
        size: a.size,
        url: a.download_link.clone(),
      })
      .collect();

    // ---- Patches (updates repos) ----
    // GitLab expects numeric group id, GitHub expects release name.
    let updates_key = if is_gitlab { release.id.to_string() } else { release.name.clone() };
    let updates_repos = match api.get_updates_repos_by_name(&updates_key).await {
      Ok(r) => r,
      Err(e) => {
        log::warn!("index: get_updates_repos_by_name('{}') failed, skipping patches: {}", &updates_key, e);
        Vec::new()
      }
    };

    let mut patches: Vec<IndexPatch> = Vec::new();
    for updates_repo in &updates_repos {
      let updates_project_id = project_id_for_api(api, updates_repo);
      let repo_releases = match api.get_repo_releases(&updates_project_id).await {
        Ok(r) => r,
        Err(e) => {
          log::warn!("index: get_repo_releases('{}') failed, skipping: {}", &updates_project_id, e);
          continue;
        }
      };

      for rr in repo_releases {
        let mut manifest_asset_url: Option<String> = None;
        let patch_assets: Vec<IndexAsset> = rr
          .assets
          .iter()
          .map(|a| {
            if a.name == MANIFEST_NAME {
              manifest_asset_url = Some(a.download_link.clone());
            }
            IndexAsset {
              name: a.name.clone(),
              size: a.size.unwrap_or(0),
              url: a.download_link.clone(),
            }
          })
          .collect();

        // Extract base_patch from the patch manifest (CDN, not rate-limited).
        let base_patch = extract_base_patch(manifest_asset_url.as_deref()).await;

        patches.push(IndexPatch {
          tag: rr.tag_name,
          base_patch,
          notes: rr.body,
          manifest: manifest_asset_url,
          assets: patch_assets,
        });
      }
    }

    // Order patches by chain (base -> newest) so that the player UI can
    // mark the first uninstalled patch as "next" in install order. The
    // provider APIs return releases newest-first, which would otherwise
    // invert the chain and mislead the user.
    let patches = order_patches_by_chain(patches);

    release_entries.push(ReleaseIndexEntry {
      name: release.name.clone(),
      path: release.path.clone(),
      tag: latest.version.clone(),
      exe_path: manifest_info.as_ref().and_then(|m| m.exe_path.clone()),
      manifest: manifest_url,
      assets,
      patches,
      total_files_count: manifest_info.as_ref().map(|m| m.total_files_count).unwrap_or(0),
      total_size: manifest_info.as_ref().map(|m| m.total_size).unwrap_or(0),
      compressed_size: manifest_info.as_ref().map(|m| m.compressed_size).unwrap_or(0),
    });
  }

  Ok(ReleaseIndex {
    schema: INDEX_SCHEMA_VERSION,
    generated_at: chrono::Utc::now().to_rfc3339(),
    launcher: launcher_index,
    presets: Vec::new(),
    users: HashMap::new(),
    releases: release_entries,
  })
}

async fn merge_existing_dev_managed_fields(api: &(dyn ApiProvider + Send + Sync), index: ReleaseIndex) -> Result<ReleaseIndex> {
  let existing = crate::service::index::load_index(api.id()).await.ok();
  Ok(apply_dev_managed_fields(index, existing))
}

/// Pure merge step, split out of `merge_existing_dev_managed_fields` so it is
/// testable without a mock `ApiProvider` (the trait has 30+ methods) or any
/// network/cache I/O: copy the dev-managed fields (`presets`, `users`) from
/// the previously published index onto a freshly collected one. A freshly
/// collected index always starts with these fields empty (see
/// `collect_release_index`) — without this step, every publish would wipe
/// out presets/user flags that are edited by hand via the raw JSON preview.
fn apply_dev_managed_fields(mut index: ReleaseIndex, existing: Option<ReleaseIndex>) -> ReleaseIndex {
  if let Some(existing_index) = existing {
    index.presets = existing_index.presets;
    index.users = existing_index.users;
  }
  index
}

/// Rebuild `index.json` from live API data and commit it to the provider's
/// index repo.  Errors are returned but callers should treat them as
/// non-fatal warnings.
pub async fn publish_index(api: &(dyn ApiProvider + Send + Sync)) -> Result<()> {
  log::info!("Publishing release index (provider: {})...", api.id());
  let index = collect_release_index(api).await?;
  let index = merge_existing_dev_managed_fields(api, index).await?;
  let content = serde_json::to_string_pretty(&index).context("index: serialize")?;
  commit_index_json(api, &content).await?;
  log::info!("Release index published for '{}' ({} releases)", api.id(), index.releases.len());
  Ok(())
}

/// Build the manifest URL for a main repo, provider-specific.
fn manifest_url_for(api: &(dyn ApiProvider + Send + Sync), main: &Project) -> String {
  if api.is_suppot_subgroups() {
    return format!(
      "{}/projects/{}/repository/files/{}/raw?ref=master",
      GITLAB_API_HOST, main.id, MANIFEST_NAME,
    );
  }

  format!("{}/{}/{}/raw/master/{}", GITHUB_HOST, GITHUB_ORG, main.name, MANIFEST_NAME,)
}

/// Download a patch manifest (CDN URL) and extract the `base_patch` field.
/// Returns `None` on any error (non-fatal — the writer should not abort).
async fn extract_base_patch(manifest_url: Option<&str>) -> Option<String> {
  let url = manifest_url?;
  let cached = crate::utils::http_cache::fetch(
    &crate::utils::http_cache::SHARED_CLIENT,
    url,
    Duration::from_secs(crate::consts::CACHE_TTL_RAW_FILE_SECS),
  )
  .await
  .ok()?;
  let manifest: ReleaseManifest = serde_json::from_slice(&cached.bytes).ok()?;
  manifest.base_patch
}

/// Choose the correct `project_id` argument for provider API calls.
/// Github uses repo name (String), Gitlab uses numeric id.
fn project_id_for_api(api: &(dyn ApiProvider + Send + Sync), project: &Project) -> String {
  if api.is_suppot_subgroups() {
    project.id.to_string()
  } else {
    project.name.clone()
  }
}

/// Download a release manifest (raw URL) and extract size fields + exe_path.
/// Returns `None` on any error (non-fatal — the index will just have 0 sizes).
async fn extract_manifest_info(manifest_url: &str) -> Option<ReleaseManifest> {
  let cached = crate::utils::http_cache::fetch(
    &crate::utils::http_cache::SHARED_CLIENT,
    manifest_url,
    Duration::from_secs(crate::consts::CACHE_TTL_RAW_FILE_SECS),
  )
  .await
  .ok()?;
  serde_json::from_slice(&cached.bytes).ok()
}

/// HEAD the launcher bg URL to capture its current ETag for the index.
async fn fetch_bg_etag(bg_url: &str) -> Option<String> {
  let resp = crate::utils::http_cache::SHARED_CLIENT.head(bg_url).send().await.ok()?;
  resp
    .headers()
    .get(reqwest::header::ETAG)
    .and_then(|v| v.to_str().ok())
    .map(|s| s.to_string())
}

fn is_main_repo(name: &str) -> bool {
  name.starts_with("main_1") || name.ends_with("main_1")
}

/// Order patches from base to newest following the `base_patch` chain.
///
/// Each patch's `base_patch` points to the tag of the patch it builds upon.
/// The root patch has `base_patch = None` (or points to a tag not present in
/// the set). Provider APIs return releases newest-first, so without this the
/// first uninstalled patch in the list would be the newest one — which the
/// player cannot install until the chain leading to it is applied.
///
/// Unresolvable / cyclic leftovers keep their original relative order appended
/// after the resolved chain, so a corrupt entry never drops a patch silently.
fn order_patches_by_chain(patches: Vec<IndexPatch>) -> Vec<IndexPatch> {
  use std::collections::HashMap;

  if patches.len() <= 1 {
    return patches;
  }

  let tags: HashMap<&str, usize> = patches.iter().enumerate().map(|(i, p)| (p.tag.as_str(), i)).collect();

  // Find the root: a patch whose base_patch is None or references a tag
  // that is not in the set (e.g. base was the game release itself).
  let mut root_idx: Option<usize> = None;
  for (i, p) in patches.iter().enumerate() {
    let is_root = match p.base_patch.as_deref() {
      None => true,
      Some(base) => !tags.contains_key(base),
    };
    if is_root {
      root_idx = Some(i);
      break;
    }
  }

  // Build child-by-base index: base_tag -> patch index.
  let mut child_by_base: HashMap<&str, usize> = HashMap::new();
  for (i, p) in patches.iter().enumerate() {
    if let Some(base) = p.base_patch.as_deref() {
      // Only link when the base is in the set (avoid stealing the root).
      if tags.contains_key(base) {
        child_by_base.insert(base, i);
      }
    }
  }

  let Some(start) = root_idx else {
    // No identifiable root (every patch references another in a cycle) —
    // leave the provider's order as-is rather than guessing.
    return patches;
  };

  let mut ordered: Vec<IndexPatch> = Vec::with_capacity(patches.len());
  let mut used = vec![false; patches.len()];
  let mut cursor = Some(start);

  while let Some(idx) = cursor {
    if used[idx] {
      break; // cycle guard
    }
    used[idx] = true;
    ordered.push(patches[idx].clone());
    cursor = child_by_base.get(patches[idx].tag.as_str()).copied();
  }

  // Append any patches that did not link into the resolved chain (broken
  // base_patch references, duplicates, etc.) in their original order.
  for (i, p) in patches.iter().enumerate() {
    if !used[i] {
      ordered.push(p.clone());
    }
  }

  ordered
}

#[cfg(test)]
mod dev_managed_fields_tests {
  use super::*;
  use std::collections::HashMap;

  fn empty_index() -> ReleaseIndex {
    ReleaseIndex {
      schema: INDEX_SCHEMA_VERSION,
      generated_at: chrono::Utc::now().to_rfc3339(),
      launcher: LauncherIndex {
        version: "0.0.0".to_string(),
        assets: vec![],
        bg_etag: None,
      },
      presets: Vec::new(),
      users: HashMap::new(),
      releases: Vec::new(),
    }
  }

  #[test]
  fn copies_presets_and_users_from_existing_index() {
    let fresh = empty_index();

    let mut existing = empty_index();
    existing.presets.push(IndexPreset {
      id: "hardcore".to_string(),
      options: HashMap::new(),
      alife: HashMap::new(),
    });
    existing.users.insert(
      "6e0ead30-48de-4421-99db-cc8b381ad0b3".to_string(),
      IndexUserData {
        flags: vec!["allowPackMod".to_string()],
      },
    );

    let merged = apply_dev_managed_fields(fresh, Some(existing));

    assert_eq!(merged.presets.len(), 1);
    assert_eq!(merged.presets[0].id, "hardcore");
    assert_eq!(merged.users.len(), 1);
    assert_eq!(
      merged.users.get("6e0ead30-48de-4421-99db-cc8b381ad0b3").unwrap().flags,
      vec!["allowPackMod".to_string()]
    );
  }

  #[test]
  fn keeps_fields_empty_when_no_existing_index() {
    // First-ever publish (or the existing index could not be loaded): a
    // freshly collected index already has empty presets/users, and there is
    // nothing to merge in — must not panic or fabricate data.
    let fresh = empty_index();
    let merged = apply_dev_managed_fields(fresh, None);

    assert!(merged.presets.is_empty());
    assert!(merged.users.is_empty());
  }

  #[test]
  fn fresh_fields_are_fully_replaced_not_merged() {
    // A freshly collected index that (hypothetically) already carried some
    // presets/users of its own must still end up with EXACTLY the existing
    // index's fields, not a union — dev-managed fields are a copy, not a merge.
    let mut fresh = empty_index();
    fresh.presets.push(IndexPreset {
      id: "stale-preset".to_string(),
      options: HashMap::new(),
      alife: HashMap::new(),
    });

    let existing = empty_index();
    let merged = apply_dev_managed_fields(fresh, Some(existing));

    assert!(merged.presets.is_empty());
  }
}
