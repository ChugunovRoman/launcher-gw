use anyhow::Result;
use urlencoding::encode;

use crate::gitlab::{
  Gitlab::Gitlab,
  models::{Issue, UserData},
};

use crate::consts::*;

pub trait GitLabClient {
  async fn get_user(&self, uuid: &str) -> Result<UserData>;
}

impl GitLabClient for Gitlab {
  async fn get_user(&self, uuid: &str) -> Result<UserData> {
    let path = format!(
      "{}/projects/{}/issues?search={}&in=title",
      self.host,
      REPO_LAUNCGER_ID,
      encode(uuid)
    );

    let response = match self.client.get(&path).send().await {
      Ok(resp) => resp,
      Err(error) => {
        log::warn!(
          "Cannot get user data from GitLab, returning default UserData. Error: {:?}",
          error
        );
        return Ok(UserData::default());
      }
    };

    if !response.status().is_success() {
      let status = response.status();
      let body = response
        .text()
        .await
        .unwrap_or_else(|_| "No response body".to_string());
      log::warn!(
        "GitLab API error {}: {}. Returning default UserData.",
        status,
        body
      );
      return Ok(UserData::default());
    }

    let issues: Vec<Issue> = match response.json().await {
      Ok(data) => data,
      Err(error) => {
        log::warn!(
          "Cannot parse issues response as JSON, returning default UserData. Error: {:?}",
          error
        );
        return Ok(UserData::default());
      }
    };

    // Ищем точное совпадение по заголовку
    let exact_match = issues.into_iter().find(|i| i.title == uuid);

    let issue = match exact_match {
      Some(issue) => issue,
      None => {
        log::warn!("UserData not found in issues, returning default UserData");
        return Ok(UserData::default());
      }
    };

    // Пытаемся десериализовать описание как UserData
    let user_data: UserData = match serde_json::from_str(&issue.description) {
      Ok(data) => data,
      Err(error) => {
        log::warn!(
          "Cannot parse issue.description as JSON, returning default UserData. Error: {:?}",
          error
        );
        return Ok(UserData::default());
      }
    };

    log::info!("User FOUND! Flags: {:?}", user_data.flags);
    Ok(user_data)
  }
}
