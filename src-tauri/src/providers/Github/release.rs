use std::{collections::HashMap, vec};

use crate::{
  consts::GITHUB_ORG,
  providers::{
    ApiProvider::ApiProvider,
    Github::{Github::Github, models::*, repo::*},
    dto::*,
  },
};

use anyhow::{Context, Result, bail};
use regex::Regex;

async fn __fetch_releases(s: &Github) -> Result<()> {
  let mut map: HashMap<u32, ProjectGithub> = HashMap::new();
  let mut page: u32 = 1;
  let release_count = s.projects_map.lock().unwrap().len();

  loop {
    if release_count > 0 {
      break;
    }

    let search_params = HashMap::from([("page".to_owned(), page.to_string()), ("per_page".to_owned(), "100".to_owned())]);
    let params = search_params.iter().map(|v| format!("{}={}", v.0, v.1)).collect::<Vec<_>>().join("&");
    let mut url = format!("{}/orgs/{}/repos", s.host, GITHUB_ORG);

    if search_params.len() > 0 {
      url = format!("{}?{}", &url, &params);
    }

    let resp = s
      .get(&url)
      .send()
      .await
      .context(format!("Failed to send request to Github (__get_releases)"))?;

    if !resp.status().is_success() {
      let status = resp.status();
      let body = resp.text().await?;
      bail!("__get_releases, Github API error ({}): {} url: {}", status, body, url);
    }

    let headers = resp.headers().clone();
    let projects: Vec<ProjectGithub> = resp.json().await?;

    if projects.len() == 0 {
      log::info!("Github __get_releases, no releases, project: {:?}", projects);
      return Ok(());
    }

    for project in projects {
      map.insert(project.id, project);
    }

    let mut has_next_page = false;

    if let Some(link) = headers.get("link") {
      if let Some(has_next) = link.to_str()?.to_string().find("next") {
        if has_next > 0 {
          has_next_page = true;
        }
      };
    };

    if !has_next_page {
      break;
    }

    page += 1;
  }

  if release_count == 0 {
    *s.projects_map.lock().unwrap() = map;
  }

  Ok(())
}

pub async fn __get_releases(s: &Github) -> Result<Vec<Release>> {
  __fetch_releases(s).await?;

  let cached_projects = s.projects_map.lock().unwrap().clone();
  let mut releases: Vec<Release> = vec![];
  let mut exist_names: HashMap<String, bool> = HashMap::new();

  for (id, project) in cached_projects {
    if let None = exist_names.get(&project.description) {
      releases.push(Release {
        id,
        name: project.description.clone(),
        path: Regex::new(r"\s+").unwrap().replace_all(&project.description, "-").to_string(),
      });
      exist_names.insert(project.description, true);
    }
  }

  Ok(releases)
}

pub async fn __get_release_repos_by_name(s: &Github, release_name: &str) -> Result<Vec<Project>> {
  __fetch_releases(s).await?;

  let cached_projects = s.projects_map.lock().unwrap().clone();
  let mut repos: Vec<Project> = vec![];

  for (id, project) in cached_projects {
    if let Some(pos) = project.name.find("_main_")
      && pos > 0
      && project.description == release_name
    {
      repos.push(Project {
        id,
        name: project.name.clone(),
        path: project.name,
        ssh_remote_url: project.ssh_url,
        marked_for_deletion_on: None,
      });
    }
  }

  Ok(repos)
}

pub async fn __get_updates_repos_by_name(s: &Github, release_name: &str) -> Result<Vec<Project>> {
  __fetch_releases(s).await?;

  let cached_projects = s.projects_map.lock().unwrap().clone();
  let mut repos: Vec<Project> = vec![];

  for (id, project) in cached_projects {
    if let Some(pos) = project.name.find("_updates_")
      && pos > 0
      && project.description == release_name
    {
      repos.push(Project {
        id,
        name: project.name.clone(),
        path: project.name,
        ssh_remote_url: project.ssh_url,
        marked_for_deletion_on: None,
      });
    }
  }

  Ok(repos)
}

pub async fn __set_release_visibility(s: &Github, release_name: &str, visibility: bool) -> Result<()> {
  __fetch_releases(s).await?;

  let cached_projects = s.projects_map.lock().unwrap().clone();

  for (_, project) in cached_projects {
    if project.description == release_name {
      __update_repo(
        s,
        &project.name,
        UpdateRepoDtoGithub {
          visibility: if visibility {
            Some("public".to_owned())
          } else {
            Some("private".to_owned())
          },
          private: Some(!visibility),
          ..UpdateRepoDtoGithub::default()
        },
      )
      .await?;
    }
  }

  Ok(())
}
