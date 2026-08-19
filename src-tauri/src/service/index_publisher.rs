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

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json;

use crate::{
    consts::*,
    handlers::dto::ReleaseManifest,
    providers::{dto::Project, ApiProvider::ApiProvider},
    service::index::*,
};

/// Rebuild `index.json` from live API data and commit it to the provider's
/// index repo.  Errors are returned but callers should treat them as
/// non-fatal warnings.
pub async fn publish_index(api: &(dyn ApiProvider + Send + Sync)) -> Result<()> {
    let is_gitlab = api.is_suppot_subgroups();

    log::info!("Publishing release index (provider: {})...", api.id());

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
    let releases_raw = api
        .get_releases(false)
        .await
        .context("index: get_releases")?;

    let mut release_entries: Vec<ReleaseIndexEntry> = Vec::new();

    for release in &releases_raw {
        let repos = match api
            .get_release_repos_by_name(&release.path)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!("index: get_release_repos_by_name('{}') failed, skipping: {}", &release.path, e);
                continue;
            }
        };

        let main_repo = repos.iter().find(|r| is_main_repo(&r.name));

        let Some(main) = main_repo else {
            log::warn!("index: no main_1 repo for release '{}', skipping", &release.path);
            continue;
        };

        let project_id = project_id_for_api(api, main);

        let latest = match api
            .get_launcher_latest_release(
                if is_gitlab { "" } else { GITHUB_ORG },
                &project_id,
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!("index: get_launcher_latest_release('{}') failed, skipping: {}", &project_id, e);
                continue;
            }
        };

        let manifest_url = manifest_url_for(api, main);

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
        let updates_repos = match api
            .get_updates_repos_by_name(&release.path)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!("index: get_updates_repos_by_name('{}') failed, skipping patches: {}", &release.path, e);
                Vec::new()
            }
        };

        let mut patches: Vec<IndexPatch> = Vec::new();
        for updates_repo in &updates_repos {
            let updates_project_id = project_id_for_api(api, updates_repo);
            let repo_releases = match api
                .get_repo_releases(&updates_project_id)
                .await
            {
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
            exe_path: None,
            manifest: manifest_url,
            assets,
            patches,
        });
    }

    let index = ReleaseIndex {
        schema: INDEX_SCHEMA_VERSION,
        generated_at: chrono::Utc::now().to_rfc3339(),
        launcher: launcher_index,
        releases: release_entries,
    };

    let content = serde_json::to_string_pretty(&index)
        .context("index: serialize")?;

    // Commit to the provider's index repo.
    if is_gitlab {
        if GITLAB_INDEX_PROJECT_ID == 0 {
            log::warn!("index: GITLAB_INDEX_PROJECT_ID = 0, skipping GitLab index publish");
            return Ok(());
        }
        api.add_file_to_repo(
            &GITLAB_INDEX_PROJECT_ID.to_string(),
            "index.json",
            &content,
            "Update release index",
            DEFAULT_BRANCH,
        )
        .await
        .context("index: add_file_to_repo (GitLab)")?;
    } else {
        api.add_file_to_repo(
            INDEX_REPO_NAME,
            "index.json",
            &content,
            "Update release index",
            DEFAULT_BRANCH,
        )
        .await
        .context("index: add_file_to_repo (GitHub)")?;
    }

    log::info!(
        "Release index published for '{}' ({} releases)",
        api.id(),
        index.releases.len()
    );
    Ok(())
}

/// Build the manifest URL for a main repo, provider-specific.
fn manifest_url_for(api: &(dyn ApiProvider + Send + Sync), main: &Project) -> String {
    if api.is_suppot_subgroups() {
        // GitLab: raw file endpoint.
        format!(
            "{}/projects/{}/repository/files/{}/raw?ref=master",
            GITLAB_API_HOST, main.id, MANIFEST_NAME,
        )
    } else {
        // GitHub: raw.githubusercontent via github.com/raw redirect.
        format!(
            "{}/{}/{}/raw/master/{}",
            GITHUB_HOST, GITHUB_ORG, main.name, MANIFEST_NAME,
        )
    }
}

/// Download a patch manifest (CDN URL) and extract the `base_patch` field.
/// Returns `None` on any error (non-fatal — the writer should not abort).
async fn extract_base_patch(manifest_url: Option<&str>) -> Option<String> {
    let url = manifest_url?;
    let client = reqwest::Client::new();
    let cached = crate::utils::http_cache::fetch(&client, url, Duration::from_secs(crate::consts::CACHE_TTL_RAW_FILE_SECS))
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

/// HEAD the launcher bg URL to capture its current ETag for the index.
async fn fetch_bg_etag(bg_url: &str) -> Option<String> {
    let client = reqwest::Client::new();
    let resp = client.head(bg_url).send().await.ok()?;
    resp.headers()
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

    let tags: HashMap<&str, usize> = patches
        .iter()
        .enumerate()
        .map(|(i, p)| (p.tag.as_str(), i))
        .collect();

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
