use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Result, bail};
use urlencoding::encode;

use crate::{
  consts::{GITHUB_LAUNCHER_REPO_NAME, MAIN_DEVELOPER_NAME, CACHE_TTL_SEARCH_API_SECS},
  providers::{
    Github::{Github::Github, models::*},
    dto::Issue,
  },
  utils::http_cache,
};

pub async fn __find_issue(s: &Github, _repo_id: &str, search_params: HashMap<String, String>) -> Result<Vec<Issue>> {
  let params = search_params
    .iter()
    .map(|v| format!("{}={}", v.0, encode(v.1)))
    .collect::<Vec<_>>()
    .join("&");

  let mut url = format!("{}/search/issues", s.host);

  if search_params.len() > 0 {
    url = format!("{}?{}", &url, &params);
  }

  let cached = s.get_cached(&url, Duration::from_secs(CACHE_TTL_SEARCH_API_SECS)).await?;
  let issues: IssueResponseGithub = serde_json::from_slice(&cached.bytes)?;

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
