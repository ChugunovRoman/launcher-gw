use anyhow::{Context, Result};
use git2::Repository;
use std::path::Path;

pub fn init_repo_with_remote(repo_path: &Path, remote_url: &str) -> Result<Repository> {
  let repo = Repository::init(repo_path).with_context(|| format!("Failed to init repo at {:?}", repo_path))?;

  repo.remote("origin", remote_url).with_context(|| "Failed to set remote")?;

  Ok(repo)
}
