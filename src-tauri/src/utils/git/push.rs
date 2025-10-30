use anyhow::{Context, Result};
use git2::{Repository, Signature};
use std::path::{Path, PathBuf};

pub fn commit_and_push(repo: &Repository, files: &[PathBuf], base_dir: &Path, author_name: &str, author_email: &str, commit_msg: &str) -> Result<()> {
  let mut index = repo.index()?;
  for file in files {
    let rel_path = file
      .strip_prefix(base_dir)
      .with_context(|| format!("File {:?} not under base dir {:?}", file, base_dir))?;
    index.add_path(rel_path)?;
  }
  let oid = index.write_tree()?;
  index.write()?;

  let signature = Signature::now(author_name, author_email).with_context(|| "Failed to create git signature")?;

  let _ = repo.head().ok().and_then(|h| h.peel_to_commit().ok());

  let tree = repo.find_tree(oid)?;
  let _commit = repo.commit(Some("HEAD"), &signature, &signature, commit_msg, &tree, &[])?;

  let mut remote = repo.find_remote("origin")?;
  remote.push(&["refs/heads/master"], None)?;

  Ok(())
}
