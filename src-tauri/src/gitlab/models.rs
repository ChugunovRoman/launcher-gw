use std::vec;

use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone)]
pub struct TreeItem {
  pub id: String,
  #[serde(skip)]
  pub project_id: u32,
  pub name: String,
  pub path: String,
  #[serde(rename = "type")]
  pub item_type: String, // "blob" или "tree"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
  pub id: u32,
  pub name: String,
  pub path: String,
  #[serde(default)]
  pub marked_for_deletion_on: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
  pub title: String,
  pub description: String,
}

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
