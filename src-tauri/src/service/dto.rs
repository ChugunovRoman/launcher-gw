use anyhow::Result;
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
