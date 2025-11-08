use anyhow::{Context, Result};
use git2::Repository;
use std::io::Write;
use std::{
  fs::{self, File},
  path::Path,
};

use crate::consts::STATE_FILE_NAME;

pub fn init_repo_with_remote(repo_path: &Path, remote_url: &str) -> Result<Repository> {
  let repo = Repository::init(repo_path).with_context(|| format!("Failed to init repo at {:?}", repo_path))?;

  repo.remote("origin", remote_url).with_context(|| "Failed to set remote")?;

  Ok(repo)
}

pub fn get_repo(repo_path: &Path) -> Result<Repository> {
  let repo = Repository::open(repo_path).with_context(|| format!("Failed to init repo at {:?}", repo_path))?;

  Ok(repo)
}

pub fn create_gitignore_with_state(repo_path: &Path) -> Result<()> {
  let gitignore_path = repo_path.join(".gitignore");

  // Проверяем, существует ли уже .gitignore — если да, не перезаписываем
  if gitignore_path.exists() {
    // Можно опционально проверить, есть ли уже строка, но для простоты пропускаем
    return Ok(());
  }

  let mut file = File::create(&gitignore_path)?;
  writeln!(file, "{}", STATE_FILE_NAME)?;
  file.sync_all()?;

  Ok(())
}

pub fn create_gitattributes<P: AsRef<Path>>(dir: P) -> Result<()> {
  let dir = dir.as_ref();
  let gitattributes_path = dir.join(".gitattributes");

  let content = "*.7z* filter=lfs diff=lfs merge=lfs -text\n";

  fs::write(gitattributes_path, content)?;
  Ok(())
}
