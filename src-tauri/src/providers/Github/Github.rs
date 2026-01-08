use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::Stream;
use rand_agents::user_agent;
use reqwest::{
  Client,
  header::{AUTHORIZATION, HeaderMap, HeaderValue},
};

use crate::{
  consts::GITHUB_PID,
  providers::{
    ApiProvider::ApiProvider,
    Github::{files::*, issues::*, launcher::*, models::*, release::*, repo::*},
    dto::{Issue, *},
  },
  service::main::LogCallback,
};

#[derive(Clone)]
pub struct Github {
  pub host: String,
  pub client: Arc<Mutex<Client>>,
  pub suppot_subgroups: bool,

  pub status: Arc<Mutex<ProviderStatus>>,
  pub manifest: Arc<Mutex<Manifest>>,
  pub projects_map: Arc<Mutex<HashMap<u32, ProjectGithub>>>,

  pub logger: LogCallback,

  token: Arc<Mutex<String>>,
}

impl Github {
  pub fn new(h: &str, suppot_subgroups: bool, logger: LogCallback) -> Result<Self> {
    let user_agent = user_agent();
    log::info!("Start init Github client with User-Agent: {}", &user_agent);

    let mut headers = HeaderMap::new();
    headers.insert("User-Agent", HeaderValue::from_str(&user_agent)?);

    let client = Client::builder().default_headers(headers).build()?;

    Ok(Self {
      host: h.to_string(),
      client: Arc::new(Mutex::new(client)),
      status: Arc::new(Mutex::new(ProviderStatus {
        available: false,
        latency_ms: None,
      })),
      suppot_subgroups,
      projects_map: Arc::new(Mutex::new(HashMap::new())),
      manifest: Arc::new(Mutex::new(Manifest { root_id: None, max_size: 0 })),
      token: Arc::new(Mutex::new("".to_owned())),
      logger,
    })
  }

  pub fn get_client(&self) -> Client {
    self.client.lock().unwrap().clone()
  }
  pub fn get(&self, url: &str) -> reqwest::RequestBuilder {
    self.get_client().get(url)
  }
  pub fn post(&self, url: &str) -> reqwest::RequestBuilder {
    self.get_client().post(url)
  }
  pub fn put(&self, url: &str) -> reqwest::RequestBuilder {
    self.get_client().put(url)
  }
  pub fn patch(&self, url: &str) -> reqwest::RequestBuilder {
    self.get_client().patch(url)
  }
  pub fn head(&self, url: &str) -> reqwest::RequestBuilder {
    self.get_client().head(url)
  }
}

#[async_trait]
impl ApiProvider for Github {
  fn set_token(&self, token: String) -> Result<()> {
    *self.token.lock().unwrap() = token.clone();

    let user_agent = user_agent();
    let mut headers = HeaderMap::new();
    let auth_value = HeaderValue::from_str(&format!("Bearer {}", token)).or_else(|_| HeaderValue::from_str(&format!("Authorization: {}", token)))?;
    headers.insert(AUTHORIZATION, auth_value);
    headers.insert("User-Agent", HeaderValue::from_str(&user_agent)?);

    *self.client.lock().unwrap() = Client::builder().default_headers(headers).build()?;

    Ok(())
  }
  fn get_token(&self) -> String {
    self.token.lock().unwrap().clone()
  }

  fn id(&self) -> &'static str {
    GITHUB_PID
  }
  async fn ping(&self) -> ProviderStatus {
    log::info!("Start PING provider: {}, url: {}", self.id(), &self.host);

    let start = Instant::now();
    let res = self
      .get(&self.host)
      .timeout(Duration::from_secs(10)) // важно: не висеть вечно
      .send()
      .await;

    let elapsed = start.elapsed();
    let latency_ms = elapsed.as_millis() as u64;
    let available = match &res {
      Ok(r) => r.status().is_success(),
      Err(_) => false,
    };
    let status_str = match &res {
      Ok(r) => format!("{}", r.status()),
      Err(e) => format!("error: {}", e),
    };

    log::info!("PING provider {} result, status: {}, ms: {}", self.id(), status_str, &latency_ms);

    let new_status = ProviderStatus {
      available,
      latency_ms: if available { Some(latency_ms) } else { None },
    };

    *self.status.lock().unwrap() = new_status.clone();
    new_status
  }
  fn status(&self) -> ProviderStatus {
    self.status.lock().unwrap().clone()
  }
  fn is_available(&self) -> bool {
    self.status().available
  }
  fn is_suppot_subgroups(&self) -> bool {
    self.suppot_subgroups
  }

  async fn load_manifest(&self) -> Result<()> {
    __load_manifest(self).await
  }
  fn get_manifest(&self) -> Result<Manifest> {
    let manifest = self.manifest.lock().unwrap().clone();

    Ok(Manifest {
      root_id: if manifest.root_id.is_some() {
        Some(manifest.root_id.unwrap())
      } else {
        None
      },
      max_size: manifest.max_size,
    })
  }

  // Files API
  async fn get_file_content_size(&self, direct_url: &str) -> Result<u64> {
    __get_file_content_size(self, direct_url).await
  }
  async fn get_launcher_bg(&self) -> Result<Vec<u8>> {
    __get_launcher_bg(self).await
  }
  async fn get_file_raw(&self, project_id: &str, file_path: &str) -> Result<Vec<u8>> {
    __get_file_raw(self, project_id, file_path).await
  }
  async fn get_blob_stream(
    &self,
    project_id: &str,
    blob_sha: &str,
    seek: &Option<u64>,
  ) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Unpin + Send>> {
    __get_blob_stream(self, project_id, blob_sha, seek).await
  }
  async fn get_blob_direct_url(&self, project_id: &str, blob_sha: &str) -> String {
    __get_blob_direct_url(self, project_id, blob_sha).await
  }
  async fn get_blob_by_url_stream(&self, link: &str, seek: &Option<u64>) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Unpin + Send>> {
    __get_blob_by_url_stream(self, link, seek).await
  }
  async fn tree(&self, repo_id: &str, search_params: HashMap<String, String>) -> Result<Vec<TreeItem>> {
    __tree(self, repo_id, search_params).await
  }
  async fn get_full_tree(&self, repo_id: String) -> Result<Vec<TreeItem>> {
    __get_full_tree(self, &repo_id).await
  }

  // Issues API
  async fn find_issue(&self, repo_id: &str, search_params: HashMap<String, String>) -> Result<Vec<Issue>> {
    __find_issue(self, repo_id, search_params).await
  }
  async fn find_user(&self, repo_id: &str, uuid: &str) -> Result<Option<Issue>> {
    __find_user(self, repo_id, uuid).await
  }

  // Repo API
  async fn create_repo(&self, name: &str, description: &str, parent_id: &str) -> Result<CreateRepoResponse> {
    __create_repo(self, name, description).await
  }

  // Groups API
  async fn create_group(&self, name: &str, parent_id: &u32) -> Result<CreategGroupResponse> {
    // Not applicable for Github
    Ok(CreategGroupResponse {
      id: 0,
      name: "".to_owned(),
      path: "".to_owned(),
      lfs_enabled: false,
      parent_id: 0,
    })
  }

  // Release
  async fn get_launcher_latest_release(&self, project_id: &str) -> Result<ReleaseGit> {
    __get_launcher_latest_release(self, project_id).await
  }
  async fn get_releases(&self, cashed: bool) -> Result<Vec<Release>> {
    __get_releases(self, cashed).await
  }
  async fn set_release_visibility(&self, release_name: &str, visibility: bool) -> Result<()> {
    __set_release_visibility(self, release_name, visibility).await
  }
  async fn get_release_repos_by_name(&self, release_id: &str) -> Result<Vec<Project>> {
    __get_release_repos_by_name(self, release_id).await
  }
  async fn get_updates_repos_by_name(&self, release_name: &str) -> Result<Vec<Project>> {
    __get_updates_repos_by_name(self, release_name).await
  }

  fn clone_box(&self) -> Box<dyn ApiProvider + Send + Sync> {
    Box::new(self.clone())
  }
}
