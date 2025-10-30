use serde::{Deserialize, Serialize};

#[derive(Deserialize, Debug, Clone)]
pub struct ManifestGitlab {
  #[serde(default)]
  pub root_id: Option<u32>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TreeItemGitlab {
  pub id: String,
  #[serde(skip)]
  pub name: String,
  pub path: String,
  #[serde(rename = "type")]
  pub item_type: String, // "blob" или "tree"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectGitlab {
  pub id: u32,
  pub name: String,
  pub path: String,
  pub ssh_url_to_repo: String,
  #[serde(default)]
  pub marked_for_deletion_on: Option<String>,
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
pub struct IssueGitlab {
  pub title: String,
  pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepoBodyGitlab {
  pub name: String,
  pub path: String,
  pub visibility: String,
  pub lfs_enabled: bool,
  pub namespace_id: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepoResponseGitlab {
  pub id: u32,
  pub name: String,
  pub path: String,
  pub ssh_url_to_repo: String,
  pub visibility: String,
  pub lfs_enabled: bool,
  pub namespace_id: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreategGroupBodyGitlab {
  pub name: String,
  pub path: String,
  pub visibility: String,
  pub lfs_enabled: bool,
  pub parent_id: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreategGroupResponseGitlab {
  pub id: u32,
  pub name: String,
  pub path: String,
  pub lfs_enabled: bool,
  pub parent_id: u32,
}
