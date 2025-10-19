use reqwest::{
  Client,
  header::{AUTHORIZATION, HeaderMap, HeaderValue},
};

#[derive(Debug, Clone)]
pub struct Gitlab {
  pub host: String,

  client: Client,
}

impl Gitlab {
  pub fn new(h: &str, b: &str) -> Result<Self, Box<dyn std::error::Error>> {
    log::info!("Satrt init Gitlab client");
    let mut headers = HeaderMap::new();
    let auth_value = HeaderValue::from_str(&format!("Bearer {}", b))
      .or_else(|_| HeaderValue::from_str(&format!("PRIVATE-TOKEN {}", b)))?;
    headers.insert(AUTHORIZATION, auth_value);

    // Опционально: добавьте User-Agent
    headers.insert(
      reqwest::header::USER_AGENT,
      HeaderValue::from_static("my-tauri-app/1.0"),
    );

    let client = Client::builder().default_headers(headers).build()?;

    Ok(Self {
      host: h.to_string(),
      client,
    })
  }

  pub async fn get_file_raw(
    &self,
    project_id: &str,
    branch: &str,
    file_path: &str,
  ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let url = format!(
      "{}/projects/{}/repository/files/{}/raw?ref={}",
      self.host, project_id, file_path, branch
    );
    let resp = self.client.get(&url).send().await?;

    if resp.status().is_success() {
      Ok(resp.bytes().await?.to_vec())
    } else {
      let status = resp.status();
      let body = resp.text().await.unwrap_or_else(|_| "No body".to_string());
      Err(format!("GitLab API error {}: {}", status, body).into())
    }
  }

  pub async fn get_launcher_bg(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    self
      .get_file_raw("75230492", "master", "src%2Fstatic%2Fbg.jpg")
      .await
  }
}
