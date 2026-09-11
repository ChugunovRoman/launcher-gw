use crate::{
  service::{dto::UserData, index::{IndexUserData, ReleaseIndex}, main::Service},
};
use anyhow::Result;

/// Build `UserData` for a specific uuid from an already-loaded release index.
fn user_data_from_index(index: &ReleaseIndex, uuid: &str) -> UserData {
  match index.users.get(uuid) {
    Some(u) => UserData {
      uuid: uuid.to_string(),
      flags: u.flags.clone(),
    },
    None => UserData::default(),
  }
}

/// Synchronous fast path for the startup pre-fill: read the last cached
/// `index.json` from disk without any network and extract the user flags.
/// Mirrors the old `AppConfig.user_data_cache` pre-fill, but uses the same
/// stale-fallback cache as the rest of the index reader.
pub fn cached_user_data_sync(provider_id: &str, uuid: &str) -> Option<UserData> {
  let url = crate::service::index::index_raw_url(provider_id).ok()?;
  let bytes = crate::utils::http_cache::read_body(&url)?;
  let index: ReleaseIndex = serde_json::from_slice(&bytes).ok()?;
  Some(user_data_from_index(&index, uuid))
}

pub trait ServiceClient {
  async fn get_user(&self, uuid: String) -> Result<UserData>;
}

impl ServiceClient for Service {
  /// `Err` here means the index itself could not be resolved (no current
  /// provider, or `load_index` failed with no stale cache to fall back to)
  /// — a real "we don't know this player's flags" outcome, not merely "not
  /// found in the index" (that case returns `Ok(UserData::default())`, same
  /// as an unknown uuid). Callers use the `Err` case to drive the
  /// `StartupState.user_data` phase instead of assuming success.
  async fn get_user(&self, uuid: String) -> Result<UserData> {
    let api = self.api_client.current_provider().map_err(|error| {
      log::warn!("get_user: provider unavailable: {:?}", error);
      error
    })?;

    let index = crate::service::index::load_index(api.id()).await.map_err(|error| {
      log::warn!("get_user: index unavailable: {:?}", error);
      error
    })?;

    Ok(user_data_from_index(&index, &uuid))
  }
}

#[cfg(test)]
mod tests {
  use std::collections::HashMap;

  use super::*;

  fn sample_index() -> ReleaseIndex {
    let mut users = HashMap::new();
    users.insert(
      "6e0ead30-48de-4421-99db-cc8b381ad0b3".to_string(),
      IndexUserData {
        flags: vec!["allowPackMod".to_string()],
      },
    );

    ReleaseIndex {
      schema: 1,
      generated_at: chrono::Utc::now().to_rfc3339(),
      launcher: crate::service::index::LauncherIndex {
        version: "0.0.0".to_string(),
        assets: vec![],
        bg_etag: None,
      },
      presets: vec![],
      users,
      releases: vec![],
    }
  }

  #[test]
  fn user_data_from_index_known_uuid() {
    let index = sample_index();
    let data = user_data_from_index(&index, "6e0ead30-48de-4421-99db-cc8b381ad0b3");
    assert_eq!(data.uuid, "6e0ead30-48de-4421-99db-cc8b381ad0b3");
    assert_eq!(data.flags, vec!["allowPackMod".to_string()]);
  }

  #[test]
  fn user_data_from_index_unknown_uuid() {
    let index = sample_index();
    let data = user_data_from_index(&index, "unknown-uuid");
    assert_eq!(data.uuid, "");
    assert!(data.flags.is_empty());
  }
}
