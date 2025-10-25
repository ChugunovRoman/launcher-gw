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
  pub fn new(h: &str, b: &str) -> Result<Self, Box<dyn std::error::Error>> {
    log::info!("Satrt init Gitlab client");
    let mut headers = HeaderMap::new();
    let auth_value = HeaderValue::from_str(&format!("Bearer {}", b))
      .or_else(|_| HeaderValue::from_str(&format!("PRIVATE-TOKEN {}", b)))?;
    headers.insert(AUTHORIZATION, auth_value);

    let client = Client::builder().default_headers(headers).build()?;

    Ok(Self {
      host: h.to_string(),
      client,
    })
  }

  // not use for now
  pub async fn create_user(
    &self,
    login: String,
    psk: String,
  ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
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
      Err(format!("GitLab API error {}: {}", status, body).into())
    }
  }

  pub fn hash_string(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    format!("{:x}", result)
  }
}
