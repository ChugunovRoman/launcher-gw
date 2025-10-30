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
    Gitlab::{create::*, files::*, issues::*, models::ManifestGitlab, release::*},
    dto::{Issue, *},
  },
};

pub struct Gitlab {
  pub host: String,
  pub client: Arc<Mutex<Client>>,
  pub suppot_subgroups: bool,

  pub status: Arc<Mutex<ProviderStatus>>,
  pub manifest: Arc<Mutex<ManifestGitlab>>,
}

impl Gitlab {
  pub fn new(h: &str, suppot_subgroups: bool) -> Result<Self> {
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
      manifest: Arc::new(Mutex::new(ManifestGitlab { root_id: None })),
    })
  }

  pub fn get_client(&self) -> Client {
    self.client.lock().unwrap().clone()
  }
}

#[async_trait]
impl ApiProvider for Gitlab {
  fn set_token(&self, token: String) -> Result<()> {
    let mut headers = HeaderMap::new();
    let auth_value = HeaderValue::from_str(&format!("Bearer {}", token)).or_else(|_| HeaderValue::from_str(&format!("PRIVATE-TOKEN {}", token)))?;
    headers.insert(AUTHORIZATION, auth_value);

    *self.client.lock().unwrap() = Client::builder().default_headers(headers).build()?;

    Ok(())
  }

  fn id(&self) -> &'static str {
    "gitlab"
  }
  async fn ping(&self) -> ProviderStatus {
    log::info!("Start PING provider: {}, url: {}", self.id(), &self.host);

    let start = Instant::now();
    let res = self
      .get_client()
      .get(format!("{}/projects/{}", &self.host, REPO_LAUNCGER_ID))
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
  async fn get_release_repos(&self, release_id: u32) -> Result<Vec<Project>> {
    __get_release_repos(self, release_id).await
  }
  async fn get_updates_repos(&self, release_id: u32) -> Result<Vec<Project>> {
    __get_updates_repos(self, release_id).await
  }
}
