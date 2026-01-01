use crate::providers::{
  Gitlab::{
    Gitlab::Gitlab,
    models::{CreategGroupBodyGitlab, CreategGroupResponseGitlab, UpdateGroupDtoGitlab, Visibility},
  },
  dto::CreategGroupResponse,
};

use anyhow::{Context, Result, bail};
use regex::Regex;

pub async fn __create_group(s: &Gitlab, name: &str, parent_id: &u32) -> Result<CreategGroupResponse> {
  let url = format!("{}/groups", s.host);
  let data = CreategGroupBodyGitlab {
    name: name.to_owned(),
    path: Regex::new(r"\s+").unwrap().replace_all(name, "-").to_string(),
    lfs_enabled: true,
    visibility: Visibility::Private,
    parent_id: parent_id.clone(),
  };

  let resp = s.post(&url).json(&data).send().await.context(format!(
    "Failed to send request to GitLab (create_repo) name: {}, parent_id: {}",
    name, parent_id
  ))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await?;
    bail!("__create_group, GitLab API error ({}): {} url: {}", status, body, url);
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

pub async fn __update_group(s: &Gitlab, group_id: &u32, data: UpdateGroupDtoGitlab) -> Result<()> {
  let url = format!("{}/groups/{}", s.host, &group_id);

  let resp = s
    .put(&url)
    .json(&data)
    .send()
    .await
    .context(format!("Failed to send request to GitLab (update_group) group_id: {}", &group_id))?;

  if !resp.status().is_success() {
    let status = resp.status();
    let body = resp.text().await?;
    bail!("__update_group, GitLab API error ({}): {} url: {}", status, body, url);
  }

  Ok(())
}
