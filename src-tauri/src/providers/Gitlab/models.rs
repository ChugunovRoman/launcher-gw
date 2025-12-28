use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
  Private,
  Internal,
  Public,
}
impl Visibility {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Public => "public",
      Self::Private => "private",
      Self::Internal => "internal",
    }
  }
}
impl fmt::Display for Visibility {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(self.as_str())
  }
}

#[derive(Deserialize, Debug, Clone)]
pub struct ManifestGitlab {
  #[serde(default)]
  pub root_id: Option<u32>,
  pub max_size: u64,
}

#[derive(Deserialize, Debug, Clone)]
pub struct TreeItemGitlab {
  pub id: String,
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
pub struct ReleaseAssetLinkGitlab {
  pub id: u32,
  pub name: String,
  pub url: String,
  pub direct_asset_url: String,
  pub link_type: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseAssetGitlab {
  pub count: u32,
  pub links: Vec<ReleaseAssetLinkGitlab>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseGitlab {
  pub name: String,
  pub tag_name: String,
  pub assets: ReleaseAssetGitlab,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueAuthorGitlab {
  pub id: u32,
  pub username: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueGitlab {
  pub title: String,
  pub description: String,
  pub author: IssueAuthorGitlab,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepoBodyGitlab {
  pub name: String,
  pub path: String,
  pub visibility: Visibility,
  pub lfs_enabled: bool,
  pub namespace_id: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepoResponseNamespaceGitlab {
  pub id: u32,
  pub name: String,
  pub path: String,
  pub kind: String,
  pub parent_id: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateRepoResponseGitlab {
  pub id: u32,
  pub name: String,
  pub path: String,
  pub ssh_url_to_repo: String,
  pub visibility: Visibility,
  pub lfs_enabled: bool,
  pub namespace: CreateRepoResponseNamespaceGitlab,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateRepoDtoGitlab {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub description: Option<String>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub path: Option<String>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub visibility: Option<Visibility>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub emails_enabled: Option<bool>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub lfs_enabled: Option<bool>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub wiki_enabled: Option<bool>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub default_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateGroupDtoGitlab {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub description: Option<String>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub path: Option<String>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub visibility: Option<Visibility>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub emails_enabled: Option<bool>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub lfs_enabled: Option<bool>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub default_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreategGroupBodyGitlab {
  pub name: String,
  pub path: String,
  pub visibility: Visibility,
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
