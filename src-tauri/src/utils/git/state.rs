use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

use crate::consts::STATE_FILE_NAME;
use fs2::FileExt;

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub enum UploadState {
  InProgress,
  Completed,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CommitSyncState {
  pub files: HashSet<String>,
  pub was_pushed: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RepoSyncState {
  pub commits: HashMap<String, CommitSyncState>,
  pub state: UploadState,
  pub total_files_count: usize,
  pub uploaded_files_count: usize,
}

// Вспомогательная функция: выполняет операцию с эксклюзивной блокировкой файла
fn with_exclusive_state_file<T, F>(repo_path: &Path, op: F) -> Result<T>
where
  F: FnOnce(&mut File) -> Result<T>,
{
  fs::create_dir_all(repo_path)?;
  let state_path = repo_path.join(STATE_FILE_NAME);
  let file = OpenOptions::new().read(true).write(true).create(true).open(&state_path)?;

  // Блокируем файл на запись (эксклюзивно)
  file
    .try_lock_exclusive()
    .map_err(|e| anyhow::anyhow!("Failed to lock state file exclusively: {}", e))?;

  let mut file_for_op = file.try_clone()?;

  let result = op(&mut file_for_op);

  // Блокировка снимется автоматически при drop

  result
}

// Вспомогательная функция: только чтение с shared-блокировкой
fn load_sync_state_locked(repo_path: &Path) -> Result<Option<RepoSyncState>> {
  let state_path = repo_path.join(STATE_FILE_NAME);
  if !state_path.exists() {
    return Ok(None);
  }

  let mut file = File::open(&state_path)?;
  file
    .try_lock_shared()
    .map_err(|e| anyhow::anyhow!("Failed to acquire shared lock on state file: {}", e))?;

  let mut content = String::new();
  file.read_to_string(&mut content)?;
  if content.is_empty() {
    return Ok(None);
  }

  let state = serde_json::from_str(&content)?;
  Ok(Some(state))
}

// === Публичные функции ===

pub fn init_sync_state(repo_path: &Path, total_files_count: usize) -> Result<()> {
  with_exclusive_state_file(repo_path, |file| {
    // Проверим, не существует ли уже состояние
    if let Some(_existing) = load_state_from_file(file)? {
      // Можно пропустить, или вернуть ошибку — зависит от логики
      // Например, пропускаем, если уже инициализировано:
      return Ok(());
    }

    let initial_state = RepoSyncState {
      commits: HashMap::new(),
      state: UploadState::InProgress,
      total_files_count,
      uploaded_files_count: 0,
    };

    save_state_to_file(file, &initial_state)?;
    Ok(())
  })
}

pub fn load_sync_state(repo_path: &Path) -> Result<Option<RepoSyncState>> {
  load_sync_state_locked(repo_path)
}

pub fn get_all_files(repo_path: &Path) -> Result<HashSet<String>> {
  let state = match load_sync_state_locked(repo_path)? {
    Some(data) => data,
    None => {
      log::info!(
        "get_all_files() Cannot find sync state file in repo: {:?} return empty HashSet",
        repo_path
      );
      return Ok(HashSet::new());
    }
  };

  let mut all_files = HashSet::new();
  for commit in state.commits.values() {
    all_files.extend(commit.files.iter().cloned());
  }
  Ok(all_files)
}

pub fn get_unpushed_commit(repo_path: &Path) -> Result<Option<(String, CommitSyncState)>> {
  let state = match load_sync_state_locked(repo_path)? {
    Some(data) => data,
    None => {
      bail!("get_unpushed_commit() Cannot find sync state file in repo: {:?}", repo_path)
    }
  };

  for (sha, commit) in &state.commits {
    if !commit.was_pushed {
      return Ok(Some((sha.clone(), commit.clone())));
    }
  }
  Ok(None)
}

pub fn add_commit(repo_path: &Path, sha: String, data: CommitSyncState) -> Result<()> {
  with_exclusive_state_file(repo_path, |file| {
    let mut state = match load_state_from_file(file)? {
      Some(s) => s,
      None => bail!("add_commit: state file is missing or empty for repo: {:?}", repo_path),
    };

    state.commits.insert(sha, data);
    save_state_to_file(file, &state)?;
    Ok(())
  })
}

pub fn set_commit_push(repo_path: &Path, sha: String, pushed: bool) -> Result<()> {
  with_exclusive_state_file(repo_path, |file| {
    let mut state = match load_state_from_file(file)? {
      Some(s) => s,
      None => bail!("set_commit_push: state file missing or empty"),
    };

    let commit = state.commits.get_mut(&sha).ok_or_else(|| anyhow::anyhow!("commit not found: {}", sha))?;

    commit.was_pushed = pushed;
    save_state_to_file(file, &state)?;
    Ok(())
  })
}
pub fn set_completed_status(repo_path: &Path) -> Result<()> {
  with_exclusive_state_file(repo_path, |file| {
    let mut state = match load_state_from_file(file)? {
      Some(s) => s,
      None => bail!("set_completed_status: state file missing or empty"),
    };

    state.state = UploadState::Completed;

    save_state_to_file(file, &state)?;
    Ok(())
  })
}

pub fn add_uploaded_files_count(repo_path: &Path, count: usize) -> Result<(usize, usize)> {
  with_exclusive_state_file(repo_path, |file| {
    let mut state = match load_state_from_file(file)? {
      Some(s) => s,
      None => bail!("add_uploaded_files_count: state file missing or empty"),
    };

    state.uploaded_files_count = state
      .uploaded_files_count
      .checked_add(count)
      .ok_or_else(|| anyhow::anyhow!("uploaded_files_count overflow"))?;

    let uploaded = state.uploaded_files_count;
    let total = state.total_files_count;

    save_state_to_file(file, &state)?;
    Ok((uploaded, total))
  })
}

// === Внутренние функции для работы с открытым файлом ===

fn load_state_from_file(file: &mut File) -> Result<Option<RepoSyncState>> {
  file.seek(SeekFrom::Start(0))?;
  let mut content = String::new();
  file.read_to_string(&mut content)?;
  if content.trim().is_empty() {
    return Ok(None);
  }
  let state = serde_json::from_str(&content)?;
  Ok(Some(state))
}

fn save_state_to_file(file: &mut File, state: &RepoSyncState) -> Result<()> {
  let content = serde_json::to_string(state)?;
  file.set_len(0)?;
  file.seek(SeekFrom::Start(0))?;
  file.write_all(content.as_bytes())?;
  file.sync_all()?;
  Ok(())
}
