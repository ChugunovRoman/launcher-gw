use std::collections::HashMap;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserData {
  pub uuid: String,
  pub flags: Vec<String>,
}

impl Default for UserData {
  fn default() -> Self {
    Self {
      uuid: "".to_string(),
      flags: vec![],
    }
  }
}

/// Persisted cache entry for `get_user` to avoid calling Search API on every
/// launcher start.  The Search API has a strict anonymous rate limit (10/min)
/// and this call was the second-largest consumer after `load_manifest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserDataCache {
  pub data: UserData,
  pub fetched_at: String, // ISO-8601 UTC
  /// `true` when `data` is the default (issue not found).  Negative results
  /// are cached with a shorter TTL so newly-issued flags appear within hours.
  pub is_negative: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindingMap {
  pub key: Option<String>,
  pub altkey: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileItem {
  pub name: String,
  pub keybinds: HashMap<String, KeybindingMap>,
}
