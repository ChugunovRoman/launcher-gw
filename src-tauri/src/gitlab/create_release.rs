use crate::gitlab::{
  Gitlab::Gitlab,
  models::{CreateRepoBody, CreateRepoResponse, CreategGroupBody, CreategGroupResponse},
};

use anyhow::{Context, Result, bail};
use regex::Regex;
use tokio::time::{Duration, sleep};

pub trait GitLabCreateRelease {
  async fn create_releases(
    &self,
    name: &str,
    parent_id: &u32,
    main_cnt: &u16,
    updates_cnt: &u16,
  ) -> Result<()>;
  async fn create_group(&self, name: &str, parent_id: &u32) -> Result<CreategGroupResponse>;
  async fn create_repo(&self, name: &str, parent_id: &u32) -> Result<CreateRepoResponse>;
}

impl GitLabCreateRelease for Gitlab {
  async fn create_releases(
    &self,
    name: &str,
    parent_id: &u32,
    main_cnt: &u16,
    updates_cnt: &u16,
  ) -> Result<()> {
    let release_group = self.create_group(name, parent_id).await?;

    for i in 1..main_cnt.to_owned() {
      let name = format!("main_{}", &i);
      let _ = self.create_repo(&name, &release_group.id).await?;

      sleep(Duration::from_millis(1000)).await;
    }

    for i in 1..updates_cnt.to_owned() {
      let name = format!("updates_{}", &i);
      let _ = self.create_repo(&name, &release_group.id).await?;

      sleep(Duration::from_millis(1000)).await;
    }

    Ok(())
  }

  async fn create_group(&self, name: &str, parent_id: &u32) -> Result<CreategGroupResponse> {
    let url = format!("{}/groups", self.host);
    let data = CreategGroupBody {
      name: name.to_owned(),
      path: Regex::new(r"\s+")
        .unwrap()
        .replace_all(name, "-")
        .to_string(),
      lfs_enabled: true,
      visibility: "public".to_owned(),
      parent_id: parent_id.clone(),
    };

    let resp = self
      .client
      .post(&url)
      .json(&data)
      .send()
      .await
      .context(format!(
        "Failed to send request to GitLab (create_repo) name: {}, parent_id: {}",
        name, parent_id
      ))?;

    if !resp.status().is_success() {
      let status = resp.status();
      let body = resp.text().await?;
      bail!("GitLab API error ({}): {}", status, body);
    }

    let result: CreategGroupResponse = resp.json().await?;

    Ok(result)
  }

  async fn create_repo(&self, name: &str, parent_id: &u32) -> Result<CreateRepoResponse> {
    let url = format!("{}/projects", self.host);
    let data = CreateRepoBody {
      name: name.to_owned(),
      path: Regex::new(r"\s+")
        .unwrap()
        .replace_all(name, "-")
        .to_string(),
      lfs_enabled: true,
      visibility: "public".to_owned(),
      namespace_id: parent_id.clone(),
    };

    let resp = self
      .client
      .post(&url)
      .json(&data)
      .send()
      .await
      .context(format!(
        "Failed to send request to GitLab (create_repo) name: {}, parent_id: {}",
        name, parent_id
      ))?;

    if !resp.status().is_success() {
      let status = resp.status();
      let body = resp.text().await?;
      bail!("GitLab API error ({}): {}", status, body);
    }

    let result: CreateRepoResponse = resp.json().await?;

    Ok(result)
  }
}
