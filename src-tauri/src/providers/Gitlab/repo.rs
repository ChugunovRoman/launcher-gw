use crate::providers::{
  Gitlab::{
    Gitlab::Gitlab,
    models::{CreateRepoBodyGitlab, CreateRepoResponseGitlab, UpdateRepoDtoGitlab, Visibility},
  },
  dto::CreateRepoResponse,
};

use anyhow::{Context, Result, bail};
use regex::Regex;

pub async fn __create_repo(s: &Gitlab, name: &str, parent_id: &u32) -> Result<CreateRepoResponse> {
  let url = format!("{}/projects", s.host);
  let data = CreateRepoBodyGitlab {
    name: name.to_owned(),
    path: Regex::new(r"\s+").unwrap().replace_all(name, "-").to_string(),
    lfs_enabled: true,
    visibility: Visibility::Private,
    namespace_id: parent_id.clone(),
  };

  let resp = s.post(&url).json(&data).send().await.context(format!(
    "Failed to send request to GitLab (create_repo) name: {}, parent_id: {}",
    name, parent_id
  ))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await?;
    bail!("__create_repo, GitLab API error ({}): {} url: {}", status, body, url);
  }

  let response_text = resp.text().await.context("Failed to read response body")?;

  let result: CreateRepoResponseGitlab =
    serde_json::from_str(&response_text).with_context(|| format!("Failed to parse GitLab response as JSON. Response body: {}", response_text))?;

  Ok(CreateRepoResponse {
    id: result.id,
    name: result.name,
    path: result.path,
    ssh_url_to_repo: result.ssh_url_to_repo,
    visibility: result.visibility.as_str().to_owned(),
    lfs_enabled: result.lfs_enabled,
    namespace_id: result.namespace.id,
  })
}

pub async fn __update_repo(s: &Gitlab, repo_id: &u32, data: UpdateRepoDtoGitlab) -> Result<()> {
  let url = format!("{}/projects/{}", s.host, &repo_id);

  let resp = s
    .put(&url)
    .json(&data)
    .send()
    .await
    .context(format!("Failed to send request to GitLab (update_repo) repo_id: {}", &repo_id))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await?;
    bail!("__update_repo, GitLab API error ({}): {} url: {}", status, body, url);
  }

  Ok(())
}
