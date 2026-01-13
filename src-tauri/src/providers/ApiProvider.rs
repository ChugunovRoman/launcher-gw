use std::collections::HashMap;

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::Stream;

use crate::providers::dto::*;

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

  async fn get_launcher_bg(&self) -> Result<Vec<u8>>;
  async fn get_file_raw(&self, project_id: &str, file_path: &str) -> Result<Vec<u8>>;
  async fn get_blob_stream(
    &self,
    project_id: &str,
    blob_sha: &str,
    seek: &Option<u64>,
  ) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Unpin + Send>>;
  async fn get_blob_direct_url(&self, project_id: &str, blob_sha: &str) -> String;
  async fn get_blob_by_url_stream(&self, link: &str, seek: &Option<u64>) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Unpin + Send>>;
  async fn tree(&self, repo_id: &str, search_params: HashMap<String, String>) -> Result<Vec<TreeItem>>;
  async fn get_full_tree(&self, repo_id: String) -> Result<Vec<TreeItem>>;
  async fn get_file_content_size(&self, direct_url: &str) -> Result<u64>;
  async fn add_file_to_repo(&self, repo_id: &str, file_name: &str, content: &str, commmit_msg: &str, branch: &str) -> Result<()>;
  async fn upload_release_file(
    &self,
    url: &str,
    content_length: u64,
    stream: Box<dyn Stream<Item = std::io::Result<Bytes>> + Send + Unpin>,
  ) -> Result<()>;

  async fn find_issue(&self, repo_id: &str, search_params: HashMap<String, String>) -> Result<Vec<Issue>>;
  async fn find_user(&self, repo_id: &str, uuid: &str) -> Result<Option<Issue>>;

  async fn create_tag(&self, repo_id: &str, tag_name: &str, branch: &str) -> Result<()>;
  async fn create_release(&self, repo_id: &str, tag_name: &str, assets: Vec<CreateReleaseAsset>) -> Result<CreateReleaseResponse>;
  fn get_asset_url(&self) -> String;

  async fn get_launcher_latest_release(&self, owner: &str, project_id: &str) -> Result<ReleaseGit>;
  async fn get_releases(&self, cashed: bool) -> Result<Vec<Release>>;
  async fn set_release_visibility(&self, release_id: &str, visibility: bool) -> Result<()>;
  async fn get_release_repos_by_name(&self, release_id: &str) -> Result<Vec<Project>>;
  async fn get_updates_repos_by_name(&self, release_name: &str) -> Result<Vec<Project>>;

  async fn create_group(&self, name: &str, parent_id: &u32) -> Result<CreategGroupResponse>;
  async fn create_repo(&self, name: &str, description: &str, parent_id: &str) -> Result<CreateRepoResponse>;

  //
  fn clone_box(&self) -> Box<dyn ApiProvider + Send + Sync>;
}
