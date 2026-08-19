use std::time::Duration;

use crate::{
  consts::{REPO_LAUNCGER_ID, USER_CACHE_TTL_POSITIVE_SECS, USER_CACHE_TTL_NEGATIVE_SECS},
  service::{dto::{UserData, UserDataCache}, main::Service},
};
use anyhow::Result;

const USER_CACHE_TTL_POSITIVE: Duration = Duration::from_secs(USER_CACHE_TTL_POSITIVE_SECS);
const USER_CACHE_TTL_NEGATIVE: Duration = Duration::from_secs(USER_CACHE_TTL_NEGATIVE_SECS);

pub trait ServiceClient {
  async fn get_user(&self, uuid: String) -> Result<UserData>;
}

impl ServiceClient for Service {
  async fn get_user(&self, uuid: String) -> Result<UserData> {
    // --- Check persisted cache first ---
    {
      let cfg = self.config.lock().await;
      if let Some(ref cached) = cfg.user_data_cache {
        if let Ok(fetched_at) = chrono::DateTime::parse_from_rfc3339(&cached.fetched_at) {
          let age = chrono::Utc::now() - fetched_at.with_timezone(&chrono::Utc);
          let ttl = if cached.is_negative { USER_CACHE_TTL_NEGATIVE } else { USER_CACHE_TTL_POSITIVE };
          if age.to_std().unwrap_or(Duration::ZERO) < ttl {
            log::info!("get_user: cache hit (negative={}, age={:?})", cached.is_negative, age);
            return Ok(cached.data.clone());
          }
        }
      }
    }

    // --- Cache miss / expired: call the API ---
    let api = match self.api_client.current_provider() {
      Ok(data) => data,
      Err(error) => {
        log::warn!("get_user: provider unavailable, returning default UserData. Error: {:?}", error);
        return Ok(UserData::default());
      }
    };

    let issues = match api.find_user(&REPO_LAUNCGER_ID.to_string(), &uuid).await {
      Ok(data) => data,
      Err(error) => {
        log::warn!("get_user: find_user failed, returning default UserData. Error: {:?}", error);
        return Ok(UserData::default());
      }
    };

    let exact_match = issues.into_iter().find(|i| i.title == uuid);

    let (user_data, is_negative) = match exact_match {
      Some(issue) => {
        match serde_json::from_str::<UserData>(&issue.description) {
          Ok(data) => {
            log::info!("User FOUND! Flags: {:?}", data.flags);
            (data, false)
          }
          Err(error) => {
            log::warn!("get_user: cannot parse issue.description as JSON, returning default. Error: {:?}", error);
            (UserData::default(), true)
          }
        }
      }
      None => {
        log::warn!("get_user: UserData not found in issues, returning default");
        (UserData::default(), true)
      }
    };

    // --- Persist to config ---
    {
      let mut cfg = self.config.lock().await;
      cfg.user_data_cache = Some(UserDataCache {
        data: user_data.clone(),
        fetched_at: chrono::Utc::now().to_rfc3339(),
        is_negative,
      });
      let _ = cfg.save();
    }

    Ok(user_data)
  }
}
