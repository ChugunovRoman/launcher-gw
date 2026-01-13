use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectGithub {
  pub id: u32,
  pub name: String,
  pub description: String,
  pub full_name: String,
  pub ssh_url: String,
  pub archived: bool,
  pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepoBodyGithub {
  pub name: String,
  pub description: String,
  pub homepage: String,
  pub private: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepoResponseOwnerGithub {
  pub login: String,
  pub id: u32,
  pub node_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepoResponseGithub {
  pub id: u32,
  pub node_id: String,
  pub name: String,
  pub description: String,
  pub full_name: String,
  pub ssh_url: String,
  pub private: bool,
  pub owner: CreateRepoResponseOwnerGithub,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAssetGithub {
  pub id: u32,
  pub size: u64,
  pub name: String,
  pub browser_download_url: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseGithub {
  pub id: u32,
  pub name: String,
  pub tag_name: String,
  pub assets: Vec<ReleaseAssetGithub>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateRepoDtoGithub {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub description: Option<String>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub homepage: Option<String>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub visibility: Option<String>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub private: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeItemGithub {
  pub name: String,
  pub path: String,
  pub sha: String,
  pub size: u32,
  #[serde(rename = "type")]
  pub file_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueGithub {
  pub id: u32,
  pub title: String,
  pub body: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueResponseGithub {
  pub total_count: u32,
  pub incomplete_results: bool,
  pub items: Vec<IssueGithub>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddFileContentBodyGithub {
  pub message: String,
  pub content: String,
  pub branch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReleaseRequestGithub {
  pub name: String,
  pub tag_name: String,
  pub target_commitish: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReleaseResponseGithub {
  pub id: u32,
  pub url: String,
  pub assets_url: String,
  pub upload_url: String,
}
