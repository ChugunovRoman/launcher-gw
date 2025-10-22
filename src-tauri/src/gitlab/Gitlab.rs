use reqwest::{
  Client,
  header::{AUTHORIZATION, HeaderMap, HeaderValue},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use urlencoding::encode;

#[derive(Debug)]
pub enum GitlabError {
  UserNotFound(String),
  InvalidUserData(String),
  ApiError(String),
}

#[derive(Debug, Clone)]
pub struct Gitlab {
  pub host: String,

  client: Client,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
  pub title: String,
  pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserData {
  pub uuid: String,
  pub flags: Vec<String>,
}

impl fmt::Display for GitlabError {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      GitlabError::UserNotFound(uuid) => write!(f, "User not found: {}", uuid),
      GitlabError::InvalidUserData(desc) => {
        write!(f, "Invalid user data in issue description: {}", desc)
      }
      GitlabError::ApiError(msg) => write!(f, "GitLab API error: {}", msg),
    }
  }
}

impl std::error::Error for GitlabError {}

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

  pub async fn get_user(&self, uuid: &str) -> Result<UserData, Box<dyn std::error::Error>> {
    let path = format!(
      "{}/projects/75230492/issues?search={}&in=title",
      self.host,
      encode(uuid)
    );
    let response = match self.client.get(path).send().await {
      Ok(resp) => resp,
      Err(error) => {
        log::warn!(
          "Cannot get user data from Gitlab, return default UserData, error: {:?}",
          error
        );

        return Ok(UserData {
          uuid: "".to_string(),
          flags: vec![],
        });
      }
    };

    if !response.status().is_success() {
      let status = response.status();
      let body = response
        .text()
        .await
        .unwrap_or_else(|_| "No response body".to_string());
      log::warn!(
        "GitLab API error {}: {}. Returning default UserData.",
        status,
        body
      );

      return Ok(UserData {
        uuid: "".to_string(),
        flags: vec![],
      });
    }

    let issues: Vec<Issue> = match response.json().await {
      Ok(data) => data,
      Err(error) => {
        log::warn!(
          "Cannot parse issues response to json, return default UserData, error: {:?}",
          error
        );

        return Ok(UserData {
          uuid: "".to_string(),
          flags: vec![],
        });
      }
    };

    // Ищем точное совпадение по заголовку
    let exact_match = issues.into_iter().find(|i| i.title == uuid);

    let issue = match exact_match {
      Some(issue) => issue,
      None => {
        log::warn!("UserData not found in issues, return default UserData");

        return Ok(UserData {
          uuid: "".to_string(),
          flags: vec![],
        });
      }
    };

    // Пытаемся десериализовать описание как UserData
    let user_data: UserData = match serde_json::from_str(&issue.description) {
      Ok(data) => data,
      Err(error) => {
        log::warn!(
          "Cannot parse issue.description to json, return default UserData, error: {:?}",
          error
        );

        return Ok(UserData {
          uuid: "".to_string(),
          flags: vec![],
        });
      }
    };

    log::info!("User FOUND!, flags: {:?}", &user_data.flags);

    Ok(user_data)
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
        "{}/projects/75230492/issues?title={}&description={}",
        self.host,
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
