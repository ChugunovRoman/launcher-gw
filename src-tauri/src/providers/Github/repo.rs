use std::collections::HashMap;

use crate::{
  consts::*,
  providers::{
    Github::{Github::Github, models::*},
    dto::CreateRepoResponse,
  },
};

use anyhow::{Context, Result, bail};

pub async fn __create_repo(s: &Github, name: &str, description: &str) -> Result<CreateRepoResponse> {
  let url = format!("{}/orgs/{}/repos", s.host, GITHUB_ORG);
  let data = CreateRepoBodyGithub {
    name: name.to_owned(),
    description: description.to_owned(),
    homepage: GITHUB_HOST.to_owned(),
    private: true,
  };

  let resp = s
    .post(&url)
    .json(&data)
    .send()
    .await
    .context(format!("Failed to send request to Github (create_repo) name: {}", name))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await?;
    bail!("__create_repo, Github API error ({}): {} url: {}", status, body, url);
  }

  let response_text = resp.text().await.context("Failed to read response body")?;

  let result: CreateRepoResponseGithub =
    serde_json::from_str(&response_text).with_context(|| format!("Failed to parse Github response as JSON. Response body: {}", response_text))?;

  Ok(CreateRepoResponse {
    id: result.id,
    name: result.name.clone(),
    path: result.name,
    ssh_url_to_repo: result.ssh_url,
    visibility: if result.private { "private".to_owned() } else { "public".to_owned() },
    lfs_enabled: true,
    namespace_id: result.owner.id,
  })
}

pub async fn __update_repo(s: &Github, repo_name: &str, data: UpdateRepoDtoGithub) -> Result<()> {
  let url = format!("{}/repos/{}/{}", s.host, GITHUB_ORG, repo_name);

  let resp = s
    .patch(&url)
    .json(&data)
    .send()
    .await
    .context(format!("Failed to send request to Github (update_repo) repo_name: {}", &repo_name))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await?;
    bail!("__update_repo, Github API error ({}): {} url: {}", status, body, url);
  }

  Ok(())
}
