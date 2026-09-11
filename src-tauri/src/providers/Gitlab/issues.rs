use std::collections::HashMap;

use anyhow::{Result, bail};
use urlencoding::encode;

use crate::providers::{
  Gitlab::{Gitlab::Gitlab, models::IssueGitlab},
  dto::Issue,
};

pub async fn __find_issue(s: &Gitlab, repo_id: &str, search_params: HashMap<String, String>) -> Result<Vec<Issue>> {
  let params = search_params
    .iter()
    .map(|v| format!("{}={}", v.0, encode(&v.1)))
    .collect::<Vec<_>>()
    .join("&");

  let mut path = format!("{}/projects/{}/issues", s.host, repo_id);

  if search_params.len() > 0 {
    path = format!("{}?{}", &path, &params);
  }

  // log::debug!("__find_issue, GitLab path: {}", &path);

  let response = s.get(&path).send().await?;

  if !response.status().is_success() {
    let status = response.status();
    let body = response.text().await.unwrap_or_else(|_| "No response body".to_string());
    let msg = format!("__find_issue, GitLab API error {}: {}. Returning default UserData.", status, body);
    log::warn!("{}", &msg);
    bail!(msg)
  }

  let text = response.text().await?;
  let issues: Vec<IssueGitlab> = serde_json::from_str(&text)?;
  let common: Vec<Issue> = issues
    .iter()
    .map(|issue| Issue {
      title: issue.title.to_owned(),
      description: issue.description.to_owned(),
    })
    .collect();

  Ok(common)
}

