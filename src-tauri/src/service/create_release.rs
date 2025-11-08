use std::time::Duration;

use crate::service::main::Service;
use anyhow::Result;
use regex::Regex;
use tokio::time::sleep;

pub trait ServiceRelease {
  async fn create_release_repos(&self, name: &str, parent_id: &u32, main_cnt: &u16, updates_cnt: &u16) -> Result<()>;
}

impl ServiceRelease for Service {
  async fn create_release_repos(&self, name: &str, parent_id: &u32, main_cnt: &u16, updates_cnt: &u16) -> Result<()> {
    let api = self.api_client.current_provider()?;

    if api.is_suppot_subgroups() {
      return __create_release_repos_gitlablike(self, name, parent_id, main_cnt, updates_cnt).await;
    } else {
      return __create_release_repos_githublike(self, name, parent_id, main_cnt, updates_cnt).await;
    }
  }
}

// inner methods

async fn __create_release_repos_gitlablike(s: &Service, name: &str, parent_id: &u32, main_cnt: &u16, updates_cnt: &u16) -> Result<()> {
  let api = s.api_client.current_provider()?;

  (s.logger)(&format!(
    "Create group for release: {} main_cnt: {} updates_cnt: {}",
    name, &main_cnt, &updates_cnt
  ));
  let release_group = api.create_group(name, parent_id).await?;
  (s.logger)(&format!("Create group for release: {} COMPLETE !", name));

  for i in 1..main_cnt.to_owned() + 1 {
    let repo_name = format!("main_{}", &i);
    (s.logger)(&format!("Create main repo for release: {} repo: {}", name, &repo_name));
    let _ = api.create_repo(&repo_name, &release_group.id).await?;
    (s.logger)(&format!("Create main repo for release: {} repo: {} COMPLETE !", name, &repo_name));

    sleep(Duration::from_millis(1000)).await;
  }

  for i in 1..updates_cnt.to_owned() + 1 {
    let repo_name = format!("updates_{}", &i);
    (s.logger)(&format!("Create main repo for release: {} repo: {}", name, &repo_name));
    let _ = api.create_repo(&repo_name, &release_group.id).await?;
    (s.logger)(&format!("Create main repo for release: {} repo: {} COMPLETE !", name, &repo_name));

    sleep(Duration::from_millis(1000)).await;
  }

  (s.logger)(&format!("Create all repos for release: {} is COMPLETED !", name));

  Ok(())
}
async fn __create_release_repos_githublike(s: &Service, name: &str, parent_id: &u32, main_cnt: &u16, updates_cnt: &u16) -> Result<()> {
  let api = s.api_client.current_provider()?;
  let code_name = Regex::new(r"\s+").unwrap().replace_all(name, "-").to_string();

  for i in 1..main_cnt.to_owned() {
    let name = format!("{}_main_{}", &code_name, &i);
    let _ = api.create_repo(&name, &parent_id).await?;

    sleep(Duration::from_millis(1000)).await;
  }

  for i in 1..updates_cnt.to_owned() {
    let name = format!("{}_updates_{}", &code_name, &i);
    let _ = api.create_repo(&name, &parent_id).await?;

    sleep(Duration::from_millis(1000)).await;
  }

  Ok(())
}
