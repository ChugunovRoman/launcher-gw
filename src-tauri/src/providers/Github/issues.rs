use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use urlencoding::encode;

use crate::{
  consts::CACHE_TTL_SEARCH_API_SECS,
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

