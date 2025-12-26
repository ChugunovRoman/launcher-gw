use std::{
  collections::HashMap,
  sync::{Arc, Mutex},
  time::{Duration, Instant},
};

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::Stream;
use reqwest::{
  Client,
  header::{AUTHORIZATION, HeaderMap, HeaderValue},
};

use crate::{
  consts::REPO_LAUNCGER_ID,
  providers::{
    ApiProvider::ApiProvider,
    Gitlab::{files::*, group::*, issues::*, models::ManifestGitlab, release::*, repo::*},
    dto::{Issue, *},
  },
  service::main::LogCallback,
};

#[derive(Clone)]
pub struct Gitlab {
  pub host: String,
  pub client: Arc<Mutex<Client>>,
  pub suppot_subgroups: bool,

  pub status: Arc<Mutex<ProviderStatus>>,
  pub manifest: Arc<Mutex<ManifestGitlab>>,

  pub logger: LogCallback,

  token: Arc<Mutex<String>>,
}

impl Gitlab {
  pub fn new(h: &str, suppot_subgroups: bool, logger: LogCallback) -> Result<Self> {
    log::info!("Satrt init Gitlab client");

    let client = Client::builder().build()?;

    Ok(Self {
      host: h.to_string(),
      client: Arc::new(Mutex::new(client)),
      status: Arc::new(Mutex::new(ProviderStatus {
        available: false,
        latency_ms: None,
      })),
      suppot_subgroups,
      manifest: Arc::new(Mutex::new(ManifestGitlab { root_id: None, max_size: 0 })),
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
}

#[async_trait]
impl ApiProvider for Gitlab {
  fn set_token(&self, token: String) -> Result<()> {
    *self.token.lock().unwrap() = token.clone();

    let mut headers = HeaderMap::new();
    let auth_value = HeaderValue::from_str(&format!("Bearer {}", token)).or_else(|_| HeaderValue::from_str(&format!("PRIVATE-TOKEN {}", token)))?;
    headers.insert(AUTHORIZATION, auth_value);

    *self.client.lock().unwrap() = Client::builder().default_headers(headers).build()?;

    Ok(())
  }
  fn get_token(&self) -> String {
    self.token.lock().unwrap().clone()
  }

  fn id(&self) -> &'static str {
    "gitlab"
  }
  async fn ping(&self) -> ProviderStatus {
    log::info!("Start PING provider: {}, url: {}", self.id(), &self.host);

    let start = Instant::now();
    let res = self
      .get(&format!("{}/projects/{}", &self.host, REPO_LAUNCGER_ID))
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
        Some(manifest.root_id.unwrap().to_string())
      } else {
        None
      },
      max_size: manifest.max_size,
    })
  }

  // Files API
  async fn get_file_raw(&self, project_id: &str, file_path: &str) -> Result<Vec<u8>> {
    __get_file_raw(self, project_id, file_path).await
  }
  async fn get_blob_stream(&self, project_id: &u32, blob_sha: &str) -> Result<Box<dyn Stream<Item = Result<Bytes>> + Unpin + Send>> {
    __get_blob_stream(self, project_id, blob_sha).await
  }
  async fn tree(&self, repo_id: u32, search_params: HashMap<String, String>) -> Result<Vec<TreeItem>> {
    __tree(self, repo_id, search_params).await
  }
  async fn get_full_tree(&self, repo_id: u32) -> Result<Vec<TreeItem>> {
    __get_full_tree(self, repo_id).await
  }

  // Issues API
  async fn find_issue(&self, repo_id: &u32, search_params: HashMap<String, String>) -> Result<Vec<Issue>> {
    __find_issue(self, repo_id, search_params).await
  }

  // Repo API
  async fn create_repo(&self, name: &str, parent_id: &u32) -> Result<CreateRepoResponse> {
    __create_repo(self, name, parent_id).await
  }

  // Groups API
  async fn create_group(&self, name: &str, parent_id: &u32) -> Result<CreategGroupResponse> {
    __create_group(self, name, parent_id).await
  }

  // Release
  async fn get_releases(&self) -> Result<Vec<Release>> {
    __get_releases(self).await
  }
  async fn set_release_visibility(&self, path: String, visibility: bool) -> Result<()> {
    __set_release_visibility(self, path, visibility).await
  }
  async fn get_release_repos_by_name(&self, release_name: String) -> Result<Vec<Project>> {
    __get_release_repos_by_name(self, release_name).await
  }
  async fn get_release_repos(&self, release_id: u32) -> Result<Vec<Project>> {
    __get_release_repos(self, release_id).await
  }
  async fn get_updates_repos(&self, release_id: u32) -> Result<Vec<Project>> {
    __get_updates_repos(self, release_id).await
  }

  fn clone_box(&self) -> Box<dyn ApiProvider + Send + Sync> {
    Box::new(self.clone())
  }
}
