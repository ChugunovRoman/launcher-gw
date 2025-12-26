use crate::{
  providers::{ApiProvider::ApiProvider, dto::ProviderStatus},
  service::main::LogCallback,
  utils::encoding::decode,
};
use anyhow::Result;
use std::collections::HashMap;

pub struct ApiClient {
  providers: HashMap<&'static str, Box<dyn ApiProvider + Send + Sync>>,
  current_provider_id: Option<&'static str>,
  pub logger: LogCallback,
}

impl ApiClient {
  pub fn new(logger: LogCallback) -> Self {
    Self {
      providers: HashMap::new(),
      current_provider_id: None,
      logger,
    }
  }

  pub fn register_provider<P: ApiProvider + 'static>(&mut self, provider: P) {
    let id = provider.id();
    self.providers.insert(id, Box::new(provider));
  }

  pub fn set_current_provider(&mut self, id: &'static str) -> anyhow::Result<()> {
    log::info!("set_current_provider, id: {}", &id);
    if self.providers.contains_key(id) {
      self.current_provider_id = Some(id);
      Ok(())
    } else {
      Err(anyhow::anyhow!("Provider '{}' not registered", id))
    }
  }

  pub fn current_provider(&self) -> anyhow::Result<&(dyn ApiProvider + Send + Sync)> {
    let id = self.current_provider_id.ok_or_else(|| anyhow::anyhow!("No current provider set"))?;
    self.get_provider(id)
  }

  pub fn get_provider_ids(&self) -> Vec<String> {
    self.providers.keys().map(|&id| id.to_string()).collect()
  }

  pub fn get_provider(&self, id: &str) -> anyhow::Result<&(dyn ApiProvider + Send + Sync)> {
    self
      .providers
      .get(id)
      .map(|p| p.as_ref())
      .ok_or_else(|| anyhow::anyhow!("Provider '{}' not registered", id))
  }

  pub fn get_status(&self, id: &str) -> anyhow::Result<ProviderStatus> {
    self
      .providers
      .get(id)
      .map(|p| p.status())
      .ok_or_else(|| anyhow::anyhow!("Provider '{}' not found", id))
  }

  /// Пингует все зарегистрированные провайдеры и обновляет их статус
  pub async fn ping_all(&self) -> Vec<(&'static str, ProviderStatus)> {
    let mut results = Vec::new();
    for (id, provider) in &self.providers {
      let status = provider.ping().await;

      log::info!(
        "Provider '{}' is {} ms: {:?}",
        &id,
        if status.available { "UP" } else { "DOWN" },
        if status.available { status.latency_ms } else { None }
      );

      results.push((*id, status));
    }
    results
  }

  /// Возвращает только доступные провайдеры
  pub fn available_providers(&self) -> Vec<&'static str> {
    self.providers.iter().filter(|(_, p)| p.is_available()).map(|(id, _)| *id).collect()
  }

  /// Возвращает список провайдеров, отсортированных по latency (быстрые — первые)
  pub fn fastest_available(&self) -> Vec<(&'static str, ProviderStatus)> {
    let mut available: Vec<_> = self
      .providers
      .iter()
      .filter_map(|(id, p)| {
        let status = p.status();
        if status.available { Some((*id, status)) } else { None }
      })
      .collect();

    available.sort_by_key(|(_, s)| s.latency_ms.unwrap_or(u64::MAX));
    available
  }

  pub async fn set_tokens(&self, tokens: HashMap<String, String>) -> Result<()> {
    for (id, token) in tokens {
      let provider = self.get_provider(&id)?;
      let decoded_value = match decode(&token) {
        Ok(decoded) => decoded,
        Err(_) => token.clone(),
      };
      provider.set_token(decoded_value)?;
    }

    Ok(())
  }
}

impl Clone for ApiClient {
  fn clone(&self) -> Self {
    let cloned_providers = self.providers.iter().map(|(id, provider)| (*id, provider.clone_box())).collect();

    Self {
      providers: cloned_providers,
      current_provider_id: self.current_provider_id,
      logger: self.logger.clone(),
    }
  }
}
