use std::{collections::HashMap, sync::Arc};

use crate::{
  configs::AppConfig::{AppConfig, Version},
  consts::{GITHUB_API_HOST, GITLAB_API_HOST},
  providers::{
    ApiClient::ApiClient::ApiClient,
    ApiProvider::ApiProvider,
    Github::Github::Github,
    Gitlab::Gitlab::Gitlab,
    dto::{ProviderStatus, Release},
  },
};
use anyhow::{Result, bail};
use tokio::sync::Mutex;

pub type LogCallback = Arc<dyn Fn(&str) + Send + Sync>;

pub struct Service {
  pub api_client: ApiClient,
  pub config: Arc<Mutex<AppConfig>>,
  pub logger: LogCallback,

  pub releases: HashMap<String, Vec<Release>>,
  pub stats: Vec<(&'static str, ProviderStatus)>,
}

impl Service {
  pub fn new(config: Arc<Mutex<AppConfig>>, logger: LogCallback) -> Self {
    Self {
      api_client: ApiClient::new(logger.clone()),
      config,
      logger,
      releases: HashMap::new(),
      stats: vec![],
    }
  }

  pub async fn register_all_providers(&mut self) -> Result<()> {
    self.register_github();
    self.register_gitlab();

    self.stats = self.api_client.ping_all().await;

    log::info!("Register providers, sorted_by_ping: {:?}", &self.stats);

    // First available provider by ping — used as fallback when the saved
    // selection is down, so the whole background init does not fail just
    // because one provider is rate-limited/unreachable.
    let first_available = self.stats.iter().find(|(_, s)| s.available).map(|(id, _)| *id);

    match {
      let cfg = self.config.lock().await;
      cfg.selected_provider_id.clone()
    } {
      Some(id) => {
        let saved_available = self
          .api_client
          .get_status(&id)
          .map(|s| s.available)
          .unwrap_or(false);

        if saved_available {
          self.api_client.set_current_provider(&id)?;
        } else if let Some(fallback_id) = first_available {
          // Runtime-only fallback: the user's saved selection stays in the
          // config and is re-tried on the next launch.
          log::warn!("Saved provider '{}' is unavailable, falling back to '{}'", &id, fallback_id);
          self.api_client.set_current_provider(fallback_id)?;
        } else {
          // Nobody is available — keep the saved selection so the error
          // message below names the user's provider.
          self.api_client.set_current_provider(&id)?;
        }
      }
      None => match first_available {
        Some(fallback_id) => {
          self.api_client.set_current_provider(fallback_id)?;
        }
        None => {
          bail!("No available API providers!");
        }
      },
    };

    Ok(())
  }

  pub async fn load_manifest(&mut self) -> Result<()> {
    let api = self.api_client.current_provider()?;

    if !api.is_available() {
      bail!("Api Provider {} is NOT available ! Cannot load manifest file !", &api.id())
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
