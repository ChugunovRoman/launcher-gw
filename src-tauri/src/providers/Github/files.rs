use anyhow::{Context, Result, bail};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use std::collections::HashMap;

use crate::{
  consts::*,
  providers::{
    Github::{Github::Github, issues::*, models::*},
    dto::{Manifest, TreeItem},
  },
};

pub async fn __get_file_raw_github(s: &Github, parent_id: &str, project_id: &str, file_path: &str) -> Result<Vec<u8>> {
  let url = format!("{}/{}/{}/raw/master/{}", GITHUB_HOST, parent_id, project_id, file_path);
  let resp = s.get(&url).send().await.context("Failed to send request to Github (get_file_raw)")?;

  if resp.status().is_success() {
    let bytes = resp.bytes().await.context("Failed to read response body")?;
    Ok(bytes.to_vec())
  } else {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("__get_file_raw, Github API error {}: {}, url: {}", status, body, url);
  }
}

pub async fn __get_file_raw(s: &Github, project_id: &str, file_path: &str) -> Result<Vec<u8>> {
  __get_file_raw_github(s, GITHUB_ORG, project_id, file_path).await
}

pub async fn __get_blob_stream(
  s: &Github,
  project_id: &str,
  file_path: &str,
  seek: &Option<u64>,
) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Unpin + Send>> {
  let url = format!("{}/{}/{}/raw/master/{}", GITHUB_HOST, GITHUB_ORG, project_id, file_path);

  __get_blob_by_url_stream(s, &url, seek).await
}
pub async fn __get_blob_direct_url(s: &Github, project_id: &str, file_path: &str) -> String {
  let url = format!("{}/{}/{}/raw/master/{}", GITHUB_HOST, GITHUB_ORG, project_id, file_path);

  url
}

pub async fn __get_blob_by_url_stream(s: &Github, url: &str, seek: &Option<u64>) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Unpin + Send>> {
  let response = match seek {
    Some(bytes) => s
      .get(url)
      .header("Range", format!("bytes={}-", bytes))
      .send()
      .await
      .context("Failed to send blob download request")?,
    None => s.get(url).send().await.context("Failed to send blob download request")?,
  };

  if !response.status().is_success() {
    let status = response.status();
    let body = response.text().await.unwrap_or_else(|_| "<failed to read response body>".to_string());
    bail!("__get_blob_by_url_stream, Error API Github: {} – {}", status, body);
  }

  Ok(Box::new(
    response.bytes_stream().map(|res| res.context("Error reading chunk from response stream")),
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
  let max_size = { s.manifest.lock().unwrap().max_size.clone() };

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

  *s.manifest.lock().unwrap() = manifest;

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
