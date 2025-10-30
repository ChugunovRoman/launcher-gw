use serde::{Deserialize, Serialize};

#[derive(Serialize, Debug, Clone)]
pub struct ProviderStatus {
  pub available: bool,
  pub latency_ms: Option<u64>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Manifest {
  pub root_id: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Release {
  pub id: u32,
  pub name: String,
  pub path: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Project {
  pub id: u32,
  pub name: String,
  pub path: String,
  pub ssh_remote_url: String,
  #[serde(default)]
  pub marked_for_deletion_on: Option<String>,
}

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
pub struct CreateRepoResponse {
  pub id: u32,
  pub name: String,
  pub path: String,
  pub ssh_url_to_repo: String,
  pub visibility: String,
  pub lfs_enabled: bool,
  pub namespace_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreategGroupResponse {
  pub id: u32,
  pub name: String,
  pub path: String,
  pub lfs_enabled: bool,
  pub parent_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
  pub title: String,
  pub description: String,
}
