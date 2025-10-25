use std::path::Path;

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use crate::{
  consts::REPO_LAUNCGER_ID,
  gitlab::{Gitlab::Gitlab, models::TreeItem},
  handlers::dto::ReleaseManifest,
};

pub trait GitLabFiles {
  async fn get_file_raw(&self, project_id: &str, branch: &str, file_path: &str) -> Result<Vec<u8>>;
  async fn get_launcher_bg(&self) -> Result<Vec<u8>>;
  async fn get_all_files_in_repo(&self, repo_id: u32) -> Result<Vec<TreeItem>>;
  async fn download_blob_to_file(
    &self,
    project_id: &u32,
    blob_sha: &str,
    output_path: impl AsRef<Path>,
  ) -> Result<()>;
  async fn fetch_manifest_from_blob(
    &self,
    project_id: &u32,
    blob_sha: &str,
  ) -> Result<ReleaseManifest>;
}

impl GitLabFiles for Gitlab {
  async fn get_file_raw(&self, project_id: &str, branch: &str, file_path: &str) -> Result<Vec<u8>> {
    let url = format!(
      "{}/projects/{}/repository/files/{}/raw?ref={}",
      self.host, project_id, file_path, branch
    );
    let resp = self
      .client
      .get(&url)
      .send()
      .await
      .context("Failed to send request to GitLab (get_file_raw)")?;

    if resp.status().is_success() {
      let bytes = resp.bytes().await.context("Failed to read response body")?;
      Ok(bytes.to_vec())
    } else {
      let status = resp.status();
      let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
      bail!("GitLab API error {}: {}", status, body);
    }
  }

  async fn get_launcher_bg(&self) -> Result<Vec<u8>> {
    self
      .get_file_raw(
        &format!("{}", REPO_LAUNCGER_ID),
        "master",
        "data%2Fbg%2Fbg.jpg",
      )
      .await
      .context("Failed to fetch launcher background")
  }

  async fn get_all_files_in_repo(&self, repo_id: u32) -> Result<Vec<TreeItem>> {
    let base_url = format!("{}/projects/{}/repository/tree", self.host, repo_id);
    let all_files = Vec::new();
    let mut page: u16 = 1;

    loop {
      let url = format!("{}?page={}&per_page=100", base_url, page);

      let resp = self
        .client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch page {} of repository tree", page))?;

      if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await?;
        bail!("GitLab API error ({}): {}", status, body);
      }

      let items: Vec<TreeItem> = resp
        .json()
        .await
        .with_context(|| format!("Failed to parse JSON on page {}", page))?;

      if items.is_empty() {
        break;
      }

      // all_files.extend(items.into_iter().filter(|item| item.item_type == "blob"));
      page += 1;
    }

    Ok(all_files)
  }

  async fn download_blob_to_file(
    &self,
    project_id: &u32,
    blob_sha: &str,
    output_path: impl AsRef<Path>,
  ) -> Result<()> {
    let url = format!(
      "{}/projects/{}/repository/blobs/{}",
      self.host, project_id, blob_sha
    );

    let response = self
      .client
      .get(&url)
      .send()
      .await
      .context("Failed to send blob download request")?;

    if !response.status().is_success() {
      let status = response.status();
      let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<failed to read response body>".to_string());
      bail!("Ошибка API GitLab: {} – {}", status, body);
    }

    let mut file = File::create(&output_path)
      .await
      .context("Failed to create output file")?;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
      let chunk = chunk.context("Error reading chunk from response stream")?;
      file
        .write_all(&chunk)
        .await
        .context("Failed to write chunk to file")?;
    }

    file.flush().await.context("Failed to flush file")?;
    Ok(())
  }

  async fn fetch_manifest_from_blob(
    &self,
    project_id: &u32,
    blob_sha: &str,
  ) -> Result<ReleaseManifest> {
    let url = format!(
      "{}/projects/{}/repository/blobs/{}",
      self.host, project_id, blob_sha
    );

    let response = self
      .client
      .get(&url)
      .send()
      .await
      .context("Failed to send blob download request")?;

    if !response.status().is_success() {
      let status = response.status();
      let body = response
        .text()
        .await
        .unwrap_or_else(|_| "<failed to read response body>".to_string());
      bail!("Ошибка API GitLab: {} – {}", status, body);
    }

    let body = response
      .text()
      .await
      .context("Failed to read blob response as text")?;

    let manifest: ReleaseManifest = serde_json::from_str(&body)
      .context("Failed to parse blob content as ReleaseManifest JSON")?;

    Ok(manifest)
  }
}
