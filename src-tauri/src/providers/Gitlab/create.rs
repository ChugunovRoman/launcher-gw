use crate::providers::{
  Gitlab::{
    Gitlab::Gitlab,
    models::{CreateRepoBodyGitlab, CreateRepoResponseGitlab, CreategGroupBodyGitlab, CreategGroupResponseGitlab},
  },
  dto::{CreateRepoResponse, CreategGroupResponse},
};

use anyhow::{Context, Result, bail};
use regex::Regex;

pub async fn __create_group(s: &Gitlab, name: &str, parent_id: &u32) -> Result<CreategGroupResponse> {
  let url = format!("{}/groups", s.host);
  let data = CreategGroupBodyGitlab {
    name: name.to_owned(),
    path: Regex::new(r"\s+").unwrap().replace_all(name, "-").to_string(),
    lfs_enabled: true,
    visibility: "public".to_owned(),
    parent_id: parent_id.clone(),
  };

  let resp = s.get_client().post(&url).json(&data).send().await.context(format!(
    "Failed to send request to GitLab (create_repo) name: {}, parent_id: {}",
    name, parent_id
  ))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await?;
    bail!("GitLab API error ({}): {}", status, body);
  }

  let result: CreategGroupResponseGitlab = resp.json().await?;

  Ok(CreategGroupResponse {
    id: result.id,
    name: result.name,
    path: result.path,
    lfs_enabled: result.lfs_enabled,
    parent_id: result.parent_id,
  })
}

pub async fn __create_repo(s: &Gitlab, name: &str, parent_id: &u32) -> Result<CreateRepoResponse> {
  let url = format!("{}/projects", s.host);
  let data = CreateRepoBodyGitlab {
    name: name.to_owned(),
    path: Regex::new(r"\s+").unwrap().replace_all(name, "-").to_string(),
    lfs_enabled: true,
    visibility: "public".to_owned(),
    namespace_id: parent_id.clone(),
  };

  let resp = s.get_client().post(&url).json(&data).send().await.context(format!(
    "Failed to send request to GitLab (create_repo) name: {}, parent_id: {}",
    name, parent_id
  ))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await?;
    bail!("GitLab API error ({}): {}", status, body);
  }

  let result: CreateRepoResponseGitlab = resp.json().await?;

  Ok(CreateRepoResponse {
    id: result.id,
    name: result.name,
    path: result.path,
    ssh_url_to_repo: result.ssh_url_to_repo,
    visibility: result.visibility,
    lfs_enabled: result.lfs_enabled,
    namespace_id: result.namespace_id,
  })
}
