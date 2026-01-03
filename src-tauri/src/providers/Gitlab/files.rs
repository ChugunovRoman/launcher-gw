use anyhow::{Context, Result, bail};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use std::collections::HashMap;

use crate::{
  consts::REPO_LAUNCGER_ID,
  providers::{
    Gitlab::{Gitlab::Gitlab, issues::*, models::TreeItemGitlab},
    dto::{Manifest, TreeItem},
  },
};

pub async fn __get_file_raw(s: &Gitlab, project_id: &str, file_path: &str) -> Result<Vec<u8>> {
  let url = format!("{}/projects/{}/repository/files/{}/raw?ref=master", s.host, project_id, file_path);
  let resp = s.get(&url).send().await.context("Failed to send request to GitLab (get_file_raw)")?;

  if resp.status().is_success() {
    let bytes = resp.bytes().await.context("Failed to read response body")?;
    Ok(bytes.to_vec())
  } else {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
    bail!("__get_file_raw, GitLab API error {}: {}, url: {}", status, body, url);
  }
}

pub async fn __get_blob_stream(s: &Gitlab, project_id: &str, blob_sha: &str) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Unpin + Send>> {
  let url = format!("{}/projects/{}/repository/blobs/{}/raw", s.host, project_id, blob_sha);

  __get_blob_by_url_stream(s, &url).await
}

pub async fn __get_blob_by_url_stream(s: &Gitlab, url: &str) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Unpin + Send>> {
  let response = s.get(url).send().await.context("Failed to send blob download request")?;

  if !response.status().is_success() {
    let status = response.status();
    let body = response.text().await.unwrap_or_else(|_| "<failed to read response body>".to_string());
    bail!("__get_blob_by_url_stream, Error API GitLab: {} – {}", status, body);
  }

  Ok(Box::new(
    response.bytes_stream().map(|res| res.context("Error reading chunk from response stream")),
  ))
}

pub async fn __tree(s: &Gitlab, repo_id: &str, search_params: HashMap<String, String>) -> Result<Vec<TreeItem>> {
  let params = search_params.iter().map(|v| format!("{}={}", v.0, v.1)).collect::<Vec<_>>().join("&");
  let mut url = format!("{}/projects/{}/repository/tree", s.host, repo_id);

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
    bail!("GitLab API error ({}): {} url: {}", status, body, url);
  }

  let items: Vec<TreeItemGitlab> = resp
    .json()
    .await
    .with_context(|| format!("Failed to parse JSON while get file liest, repo: {}, params: {}", &repo_id, &params))?;

  let common: Vec<TreeItem> = items
    .iter()
    .map(|item| TreeItem {
      id: item.id.clone(),
      project_id: repo_id.to_string(),
      name: item.name.clone(),
      path: item.path.clone(),
      item_type: item.item_type.clone(),
    })
    .collect();

  Ok(common)
}

pub async fn __get_full_tree(s: &Gitlab, repo_id: &str) -> Result<Vec<TreeItem>> {
  let mut all_files = Vec::new();
  let mut page: u16 = 1;

  loop {
    let search_params = HashMap::from([("page".to_owned(), page.to_string()), ("per_page".to_owned(), "100".to_owned())]);

    let items = __tree(s, repo_id, search_params)
      .await
      .with_context(|| format!("Failed to fetch page {} of repository tree", page))?;

    if items.is_empty() {
      break;
    }

    all_files.extend(items);

    page += 1;
  }

  Ok(all_files)
}

pub async fn __load_manifest(s: &Gitlab) -> Result<()> {
  let search_params = HashMap::from([("in".to_owned(), "title".to_owned()), ("search".to_owned(), "mainfest.json".to_owned())]);
  let issue = __find_issue(s, &REPO_LAUNCGER_ID.to_string(), search_params).await?;

  if issue.len() == 0 {
    bail!("Issue mainfest.json NOT FOUND!")
  }

  let manifest: Manifest = serde_json::from_str(&issue[0].description)?;

  *s.manifest.lock().unwrap() = manifest;

  Ok(())
}

pub async fn __get_launcher_bg(s: &Gitlab) -> Result<Vec<u8>> {
  __get_file_raw(s, &REPO_LAUNCGER_ID.to_string(), "data%2Fbg%2Fbg.jpg").await
}
