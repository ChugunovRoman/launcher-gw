use std::time::Duration;

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use std::collections::HashMap;

use crate::{
  consts::*,
  providers::{
    Github::{Github::Github, issues::*, models::*},
    dto::{BlobStreamWithOffset, Manifest, TreeItem},
  },
  utils::http_cache,
};

pub async fn __get_file_raw_github(s: &Github, parent_id: &str, project_id: &str, file_path: &str) -> Result<Vec<u8>> {
  let url = format!("{}/{}/{}/raw/master/{}", GITHUB_HOST, parent_id, project_id, file_path);
  let cached = s.get_cached(&url, Duration::from_secs(CACHE_TTL_RAW_FILE_SECS)).await?;
  Ok(cached.bytes)
}

pub async fn __get_file_raw(s: &Github, project_id: &str, file_path: &str) -> Result<Vec<u8>> {
  __get_file_raw_github(s, GITHUB_ORG, project_id, file_path).await
}

pub async fn __get_blob_stream(
  s: &Github,
  project_id: &str,
  file_path: &str,
  seek: &Option<u64>,
) -> Result<BlobStreamWithOffset> {
  let url = format!("{}/{}/{}/raw/master/{}", GITHUB_HOST, GITHUB_ORG, project_id, file_path);

  __get_blob_by_url_stream(s, &url, seek).await
}
pub async fn __get_blob_direct_url(s: &Github, project_id: &str, file_path: &str) -> String {
  let url = format!("{}/{}/{}/raw/master/{}", GITHUB_HOST, GITHUB_ORG, project_id, file_path);

  url
}

pub async fn __get_blob_by_url_stream(s: &Github, url: &str, seek: &Option<u64>) -> Result<BlobStreamWithOffset> {
  crate::utils::paths::assert_download_url_allowed(url)?;

  // Only ask for a Range when there is a real resume offset.
  let resume_from = seek.filter(|bytes| *bytes > 0);

  let response = match resume_from {
    Some(bytes) => s
      .get(url)
      .header("Range", format!("bytes={}-", bytes))
      .send()
      .await
      .context("Failed to send blob download request")?,
    None => s.get(url).send().await.context("Failed to send blob download request")?,
  };

  crate::utils::paths::assert_download_url_allowed(response.url().as_str())?;

  if !response.status().is_success() {
    let status = response.status();
    let body = response.text().await.unwrap_or_else(|_| "<failed to read response body>".to_string());
    bail!("__get_blob_by_url_stream, Error API Github: {} – {}", status, body);
  }

  // A server may ignore the Range header and answer 200 with the FULL body.
  // Appending that body at the resume offset would corrupt the file, so report
  // the real stream start and let the caller restart from scratch when needed.
  let stream_start = if let Some(bytes) = resume_from {
    if response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
      response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(crate::utils::parse_strings::parse_content_range_start)
        .unwrap_or(bytes)
    } else {
      log::warn!(
        "__get_blob_by_url_stream: server ignored Range (status {}), stream starts at 0",
        response.status()
      );
      0
    }
  } else {
    0
  };

  Ok((
    Box::new(response.bytes_stream().map(|res| res.context("Error reading chunk from response stream"))),
    stream_start,
  ))
}

pub async fn __tree(s: &Github, repo_id: &str, search_params: HashMap<String, String>) -> Result<Vec<TreeItem>> {
  let params = search_params.iter().map(|v| format!("{}={}", v.0, v.1)).collect::<Vec<_>>().join("&");
  let mut url = format!("{}/repos/{}/{}/contents", s.host, GITHUB_ORG, repo_id);

  if search_params.len() > 0 {
    url = format!("{}?{}", &url, &params);
  }

  let resp = s
    .get(&url)
    .send()
    .await
    .with_context(|| format!("Failed to fetch file list of repository {} tree, params: {:?}", &repo_id, &params))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await?;
    bail!("Github API error ({}): {} url: {}", status, body, url);
  }

  let items: Vec<TreeItemGithub> = resp
    .json()
    .await
    .with_context(|| format!("Failed to parse JSON while get file liest, repo: {}, params: {}", &repo_id, &params))?;

  let common: Vec<TreeItem> = items
    .iter()
    .map(|item| TreeItem {
      id: item.name.clone(),
      project_id: repo_id.to_string(),
      name: item.name.clone(),
      path: item.path.clone(),
      item_type: item.file_type.clone(),
    })
    .collect();

  Ok(common)
}

pub async fn __get_full_tree(s: &Github, repo_id: &str) -> Result<Vec<TreeItem>> {
  let items = __tree(s, repo_id, HashMap::new())
    .await
    .with_context(|| format!("Failed to fetch of {} repository tree", repo_id))?;

  Ok(items)
}

pub async fn __load_manifest(s: &Github) -> Result<()> {
  let max_size = { crate::utils::locks::lock(&s.manifest).max_size.clone() };

  if max_size > 0 {
    return Ok(());
  }

  let search_params = HashMap::from([(
    "q".to_owned(),
    format!(
      "mainfest.json in:title repo:{}/{} is:issue author:{}",
      MAIN_DEVELOPER_NAME, GITHUB_LAUNCHER_REPO_NAME, MAIN_DEVELOPER_NAME
    ),
  )]);
  let issue = __find_issue(s, &REPO_LAUNCGER_ID.to_string(), search_params).await?;

  if issue.len() == 0 {
    bail!("Issue mainfest.json NOT FOUND!")
  }

  let manifest: Manifest = serde_json::from_str(&issue[0].description)?;

  *crate::utils::locks::lock(&s.manifest) = manifest;

  Ok(())
}

pub async fn __get_launcher_bg(s: &Github) -> Result<Vec<u8>> {
  __get_file_raw_github(s, MAIN_DEVELOPER_NAME, GITHUB_LAUNCHER_REPO_NAME, "src%2Fstatic%2Fbg.jpg").await
}

pub async fn __get_file_content_size(s: &Github, direct_url: &str) -> Result<u64> {
  let resp = s
    .head(direct_url)
    .send()
    .await
    .context("Failed to send request to Github (__get_file_content_size)")?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("__get_file_content_size, Github API error {}: {} url: {}", status, body, direct_url);
  }

  let mut size: u64 = 0;
  if let Some(header) = resp.headers().get("content-length") {
    size = header.to_str()?.parse()?;
  };

  Ok(size)
}

pub async fn __add_file_to_repo(s: &Github, repo_id: &str, file_name: &str, content: &str, commmit_msg: &str, branch: &str) -> Result<()> {
  let url = format!("{}/repos/{}/{}/contents/{}", s.host, GITHUB_ORG, repo_id, file_name);
  let content_base64 = general_purpose::STANDARD.encode(content);

  // The GitHub Contents API requires the existing file's `sha` when updating.
  // GET the current metadata first; a 404 means the file does not exist yet
  // and we create it with a plain PUT (no `sha`).
  let existing_sha: Option<String> = {
    let resp = s
      .get(&format!("{}?ref={}", &url, branch))
      .send()
      .await
      .context("Failed to send request to Github (__add_file_to_repo GET)")?;
    if resp.status().is_success() {
      resp.json::<ContentFileGithub>().await.map(|f| Some(f.sha)).unwrap_or(None)
    } else {
      None
    }
  };

  let data = AddFileContentBodyGithub {
    content: content_base64.to_string(),
    message: commmit_msg.to_string(),
    branch: branch.to_string(),
    sha: existing_sha.clone(),
  };

  let resp = s
    .put(&url)
    .json(&data)
    .send()
    .await
    .context("Failed to send request to Github (__add_file_to_repo PUT)")?;

  if resp.status().is_success() {
    return Ok(());
  }

  // If the file disappeared between our GET and PUT (race), retry once as a
  // create (no `sha`). Non-fatal: the next publish will resolve the new sha.
  let status = resp.status();
  let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
  if existing_sha.is_some() && status == reqwest::StatusCode::UNPROCESSABLE_ENTITY {
    log::warn!("__add_file_to_repo: PUT 422 with stale sha, retrying as create (no sha)");
    let create_data = AddFileContentBodyGithub {
      content: content_base64.to_string(),
      message: commmit_msg.to_string(),
      branch: branch.to_string(),
      sha: None,
    };
    let resp2 = s.put(&url).json(&create_data).send().await
      .context("Failed to send request to Github (__add_file_to_repo retry PUT)")?;
    if resp2.status().is_success() {
      return Ok(());
    }
    let status2 = resp2.status();
    let body2 = resp2.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("__add_file_to_repo, Github API error (PUT {}: {} then retry PUT {}: {}), url: {}", status, body, status2, body2, url);
  }

  bail!("__add_file_to_repo, Github API error {}: {} data: {:?} url: {}", status, body, data, url);
}

pub async fn __upload_release_file(
  s: &Github,
  url: &str,
  content_length: u64,
  stream: Box<dyn Stream<Item = std::io::Result<Bytes>> + Send + Unpin>,
) -> Result<()> {
  let response = s
    .put(url)
    .header("Content-Type", "application/zip")
    .header("Content-Length", content_length.to_string())
    .body(reqwest::Body::wrap_stream(stream))
    .send()
    .await?;

  if response.status().is_success() {
    Ok(())
  } else {
    let status = response.status();
    let body = response.text().await.unwrap_or_else(|_| "No body".to_string());
    log::error!("{:?}", body);
    Err(anyhow::anyhow!("Upload failed, url: {}: {}", url, status))
  }
}
