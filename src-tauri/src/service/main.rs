use std::{collections::HashMap, sync::Arc};

use crate::{
  configs::AppConfig::AppConfig,
  consts::{GITHUB_API_HOST, GITHUB_PID, GITLAB_API_HOST},
  providers::{ApiClient::ApiClient::ApiClient, ApiProvider::ApiProvider, Github::Github::Github, Gitlab::Gitlab::Gitlab},
};
use anyhow::{Result, bail};
use tokio::sync::Mutex;

pub type LogCallback = Arc<dyn Fn(&str) + Send + Sync>;

pub struct Service {
  pub api_client: ApiClient,
  pub config: Arc<Mutex<AppConfig>>,
  pub logger: LogCallback,
}

impl Service {
  pub fn new(config: Arc<Mutex<AppConfig>>, logger: LogCallback) -> Self {
    Self {
      api_client: ApiClient::new(logger.clone()),
      config,
      logger,
    }
  }

  pub async fn register_all_providers(&mut self) -> Result<()> {
    self.register_github();
    self.register_gitlab();

    let sorted_by_ping = self.api_client.ping_all().await;

    log::info!("Register providers, sorted_by_ping: {:?}", &sorted_by_ping);

    // self.api_client.set_current_provider(sorted_by_ping[0].0)?;
    self.api_client.set_current_provider(GITHUB_PID)?;

    Ok(())
  }

  pub async fn load_manifest(&mut self) -> Result<()> {
    let api = self.api_client.current_provider()?;

    if !api.is_available() {
      bail!("Api Provider {} is available ! Cannot load manifest file !", &api.id())
    }

    api.load_manifest().await?;

    Ok(())
  }

  pub async fn set_tokens(&self, tokens: HashMap<String, String>) -> Result<()> {
    Ok(self.api_client.set_tokens(tokens).await?)
  }

  fn register_github(&mut self) -> Result<()> {
    let github = Github::new(GITHUB_API_HOST, false, self.logger.clone())?;
    let github_id = github.id();

    log::info!("Register provider: {}", github_id);

    self.api_client.register_provider(github);

    Ok(())
  }
  fn register_gitlab(&mut self) -> Result<()> {
    let gitlab = Gitlab::new(GITLAB_API_HOST, true, self.logger.clone())?;
    let gitlab_id = gitlab.id();

    log::info!("Register provider: {}", gitlab_id);

    self.api_client.register_provider(gitlab);

    Ok(())
  }
}
