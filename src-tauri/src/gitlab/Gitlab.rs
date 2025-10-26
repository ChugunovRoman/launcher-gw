use anyhow::{Context, Result, bail};
use reqwest::{
  Client,
  header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use sha2::{Digest, Sha256};
use urlencoding::encode;

use crate::consts::*;

#[derive(Debug, Clone)]
pub struct Gitlab {
  pub host: String,

  pub client: Client,
}

impl Gitlab {
  pub fn new(h: &str) -> Result<Self> {
    log::info!("Satrt init Gitlab client");

    let client = Client::builder().build()?;

    Ok(Self {
      host: h.to_string(),
      client,
    })
  }

  pub fn set_token(&mut self, token: String) -> Result<()> {
    let mut headers = HeaderMap::new();
    let auth_value = HeaderValue::from_str(&format!("Bearer {}", token))
      .or_else(|_| HeaderValue::from_str(&format!("PRIVATE-TOKEN {}", token)))?;
    headers.insert(AUTHORIZATION, auth_value);

    self.client = Client::builder().default_headers(headers).build()?;

    Ok(())
  }

  // not use for now
  pub async fn create_user(&self, login: String, psk: String) -> Result<Vec<u8>> {
    let data = format!(
      r#"{{"login":"{}","psk":"{}","flags":[]}}"#,
      &login,
      &Self::hash_string(&psk)
    );
    let resp = self
      .client
      .post(format!(
        "{}/projects/{}/issues?title={}&description={}",
        self.host,
        REPO_LAUNCGER_ID,
        encode(&login),
        encode(&data)
      ))
      .send()
      .await?;

    if resp.status().is_success() {
      Ok(resp.bytes().await?.to_vec())
    } else {
      let status = resp.status();
      let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
      bail!("GitLab API error {}: {}", status, body)
    }
  }

  pub fn hash_string(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
  }
}
