use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::Stream;

use crate::providers::{Gitlab::models::ReleaseGitlab, dto::*};

#[async_trait]
pub trait ApiProvider: Send + Sync {
  /// Уникальный идентификатор провайдера: "gitlab", "github", и т.д.
  fn id(&self) -> &'static str;

  /// Проверяет доступность сервиса (например, делает HEAD-запрос к корню API)
  async fn ping(&self) -> ProviderStatus;

  /// Возвращает текущий статус доступности (может быть кэшированным)
  fn status(&self) -> ProviderStatus;
  fn is_available(&self) -> bool;

  fn is_suppot_subgroups(&self) -> bool;

  fn set_token(&self, token: String) -> Result<()>;
  fn get_token(&self) -> String;

  async fn load_manifest(&self) -> Result<()>;
  fn get_manifest(&self) -> Result<Manifest>;

  async fn get_file_raw(&self, project_id: &str, file_path: &str) -> Result<Vec<u8>>;
  async fn get_blob_stream(&self, project_id: &u32, blob_sha: &str) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Unpin + Send>>;
  async fn get_blob_by_url_stream(&self, link: &str) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Unpin + Send>>;
  async fn tree(&self, repo_id: u32, search_params: HashMap<String, String>) -> Result<Vec<TreeItem>>;
  async fn get_full_tree(&self, repo_id: u32) -> Result<Vec<TreeItem>>;

  async fn find_issue(&self, repo_id: &u32, search_params: HashMap<String, String>) -> Result<Vec<Issue>>;

  async fn get_launcher_latest_release(&self, project_id: u32) -> Result<ReleaseGitlab>;
  async fn get_releases(&self) -> Result<Vec<Release>>;
  async fn set_release_visibility(&self, path: String, visibility: bool) -> Result<()>;
  async fn get_release_repos_by_name(&self, release_name: String) -> Result<Vec<Project>>;
  async fn get_release_repos(&self, release_id: u32) -> Result<Vec<Project>>;
  async fn get_updates_repos(&self, release_id: u32) -> Result<Vec<Project>>;

  async fn create_group(&self, name: &str, parent_id: &u32) -> Result<CreategGroupResponse>;
  async fn create_repo(&self, name: &str, parent_id: &u32) -> Result<CreateRepoResponse>;

  //
  fn clone_box(&self) -> Box<dyn ApiProvider + Send + Sync>;
}
