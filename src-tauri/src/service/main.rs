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
/// Shared provider stats — lives outside `Service` so reads never block behind
/// the Service lock during long network operations.
pub type ProviderStats = Arc<Mutex<Vec<(&'static str, ProviderStatus)>>>;


pub struct Service {
  pub api_client: ApiClient,
  pub config: Arc<Mutex<AppConfig>>,
  pub logger: LogCallback,

  pub releases: HashMap<String, Vec<Release>>,
}

impl Service {
  pub fn new(config: Arc<Mutex<AppConfig>>, logger: LogCallback) -> Self {
    Self {
      api_client: ApiClient::new(logger.clone()),
      config,
      logger,
      releases: HashMap::new(),
    }
  }

  /// Register providers locally (no network) and select the current one from
  /// the saved config value.  Falls back to "github" when the saved id is
  /// absent or unknown.  Never fails — the launcher starts offline on cache.
  /// Synchronous: only does HashMap inserts, so it is safe to call from the
  /// sync part of `tauri_setup` (before the config is wrapped in an Arc).
  pub fn register_providers_local(&mut self, selected_provider_id: Option<&str>) {
    let _ = self.register_github();
    let _ = self.register_gitlab();

    let provider_id = match selected_provider_id {
      Some(id) if self.api_client.get_provider(id).is_ok() => id.to_string(),
      _ => {
        log::warn!(
          "register_providers_local: saved provider {:?} not registered, defaulting to 'github'",
          selected_provider_id
        );
        "github".to_string()
      }
    };

    if let Err(e) = self.api_client.set_current_provider(&provider_id) {
      log::error!("register_providers_local: cannot set current provider '{}': {}", provider_id, e);
    }
  }

  /// Legacy blocking init: register + ping + select.  Kept for reference but
  /// no longer called from `tauri_setup`.
  pub async fn register_all_providers(&mut self) -> Result<()> {
    self.register_github();
    self.register_gitlab();

    let stats = self.api_client.ping_all().await;

    log::info!("Register providers, sorted_by_ping: {:?}", &stats);

    let first_available = stats.iter().find(|(_, s)| s.available).map(|(id, _)| *id);

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
          log::warn!("Saved provider '{}' is unavailable, falling back to '{}'", &id, fallback_id);
          self.api_client.set_current_provider(fallback_id)?;
        } else {
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

/// Ping all providers on a cloned `ApiClient` (no `Service` lock held).
/// Returns the sorted stats and the best available provider id.
pub async fn refresh_provider_stats(api_client: &ApiClient) -> (Vec<(&'static str, ProviderStatus)>, Option<String>) {
  let stats = api_client.ping_all().await;
  log::info!("refresh_provider_stats: {:?}", &stats);

  let best = stats.iter().find(|(_, s)| s.available).map(|(id, _)| id.to_string());
  (stats, best)
}

#[cfg(test)]
mod tests {
  use super::*;

  fn make_service() -> Service {
    // Deserialize from an empty object: every field has a serde default, and
    // this skips AppConfig::default() which probes display resolutions via
    // winit (panics outside the main thread on Windows).
    let config: AppConfig = serde_json::from_str("{}").unwrap();
    let config = Arc::new(Mutex::new(config));
    let logger: LogCallback = Arc::new(|_| {});
    Service::new(config, logger)
  }

  #[test]
  fn register_providers_local_selects_saved_provider() {
    let mut svc = make_service();
    svc.register_providers_local(Some("gitlab"));
    let current = svc.api_client.current_provider().expect("current provider must be set");
    assert_eq!(current.id(), "gitlab");
  }

  #[test]
  fn register_providers_local_unknown_saved_falls_back_to_github() {
    let mut svc = make_service();
    svc.register_providers_local(Some("unknown-provider"));
    let current = svc.api_client.current_provider().expect("current provider must be set");
    assert_eq!(current.id(), "github");
  }

  #[test]
  fn register_providers_local_no_selection_defaults_to_github() {
    let mut svc = make_service();
    svc.register_providers_local(None);
    let current = svc.api_client.current_provider().expect("current provider must be set");
    assert_eq!(current.id(), "github");
  }

  #[test]
  fn register_providers_local_registers_both_providers() {
    let mut svc = make_service();
    svc.register_providers_local(None);
    let ids = svc.api_client.get_provider_ids();
    assert!(ids.contains(&"github".to_string()));
    assert!(ids.contains(&"gitlab".to_string()));
  }
}
