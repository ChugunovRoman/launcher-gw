use std::collections::HashMap;

use anyhow::{Result, bail};
use urlencoding::encode;

use crate::{
  consts::{GITHUB_LAUNCHER_REPO_NAME, MAIN_DEVELOPER_NAME},
  providers::{
    Github::{Github::Github, models::*},
    dto::Issue,
  },
};

pub async fn __find_issue(s: &Github, repo_id: &str, search_params: HashMap<String, String>) -> Result<Vec<Issue>> {
  let params = search_params
    .iter()
    .map(|v| format!("{}={}", v.0, encode(v.1)))
    .collect::<Vec<_>>()
    .join("&");

  let mut path = format!("{}/search/issues", s.host);

  if search_params.len() > 0 {
    path = format!("{}?{}", &path, &params);
  }

  // log::debug!("Github __find_issue, path: {}", &path);

  let response = s.get(&path).send().await?;

  if !response.status().is_success() {
    let status = response.status();
    let body = response.text().await.unwrap_or_else(|_| "No response body".to_string());
    let msg = format!("__find_issue, Github API error {}: {}. Returning default UserData.", status, body);
    log::warn!("{}", &msg);
    bail!(msg)
  }

  let text = response.text().await?;
  // log::info!("Get github Issues: {}", &text);
  let issues: IssueResponseGithub = serde_json::from_str(&text)?;

  if issues.total_count == 0 {
    return Ok(vec![]);
  }

  let common: Vec<Issue> = issues
    .items
    .iter()
    .map(|issue| Issue {
      title: issue.title.to_owned(),
      description: issue.body.to_owned(),
    })
    .collect();

  Ok(common)
}

pub async fn __find_user(s: &Github, repo_id: &str, uuid: &str) -> Result<Option<Issue>> {
  let search_params = HashMap::from([(
    "q".to_owned(),
    format!(
      "{} in:title repo:{}/{} is:issue author:{}",
      uuid, MAIN_DEVELOPER_NAME, GITHUB_LAUNCHER_REPO_NAME, MAIN_DEVELOPER_NAME
    ),
  )]);

  let issues = __find_issue(s, repo_id, search_params).await?;

  if issues.len() > 0 {
    return Ok(Some(issues[0].clone()));
  }

  Ok(None)
}
