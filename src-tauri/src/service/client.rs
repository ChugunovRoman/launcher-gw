use crate::{
  consts::REPO_LAUNCGER_ID,
  service::{dto::UserData, main::Service},
};
use anyhow::Result;

pub trait ServiceClient {
  async fn get_user(&self, uuid: String) -> Result<UserData>;
}

impl ServiceClient for Service {
  async fn get_user(&self, uuid: String) -> Result<UserData> {
    let api = match self.api_client.current_provider() {
      Ok(data) => data,
      Err(error) => {
        log::warn!("Cannot parse issues response as JSON, returning default UserData. Error: {:?}", error);
        return Ok(UserData::default());
      }
    };

    let issues = match api.find_user(&REPO_LAUNCGER_ID.to_string(), &uuid).await {
      Ok(data) => data,
      Err(error) => {
        log::warn!("Cannot parse issues response as JSON, returning default UserData. Error: {:?}", error);
        return Ok(UserData::default());
      }
    };

    // Ищем точное совпадение по заголовку
    // log::debug!("exact_match, uuid: {} issues: {:?}", &uuid, &issues);
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
        log::warn!("Cannot parse issue.description as JSON, returning default UserData. Error: {:?}", error);
        return Ok(UserData::default());
      }
    };

    log::info!("User FOUND! Flags: {:?}", user_data.flags);
    Ok(user_data)
  }
}
