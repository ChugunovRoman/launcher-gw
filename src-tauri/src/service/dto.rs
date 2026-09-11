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
