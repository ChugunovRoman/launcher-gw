use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::errors::log_full_error;
use crate::utils::git::state::get_all_files;

#[derive(Debug)]
pub struct FileGroup {
  pub files: Vec<PathBuf>,
  pub total_size: u64,
}

pub fn group_files_by_size(dir: &Path, max_size: u64) -> Result<Vec<FileGroup>> {
  let mut groups = Vec::new();
  let mut current_group = FileGroup {
    files: Vec::new(),
    total_size: 0,
  };

  for entry in fs::read_dir(dir).with_context(|| format!("Failed to read directory: {:?}", dir))? {
    let entry = entry?;
    let path = entry.path();

    if !path.is_file() {
      continue;
    }
    let base_name = match path.file_name() {
      Some(name) => name.to_string_lossy(),
      None => continue,
    };

    if !base_name.starts_with("game.7z") {
      continue;
    }

    let metadata = fs::metadata(&path)?;
    let file_size = metadata.len();

    if current_group.total_size + file_size > max_size && !current_group.files.is_empty() {
      groups.push(current_group);
      current_group = FileGroup {
        files: vec![path],
        total_size: file_size,
      };
    } else {
      current_group.files.push(path);
      current_group.total_size += file_size;
    }
  }

  if !current_group.files.is_empty() {
    groups.push(current_group);
  }

  Ok(groups)
}

pub fn get_existing_groups(dir: &Path) -> Result<Vec<FileGroup>> {
  let mut groups = Vec::new();

  for entry_dir in fs::read_dir(dir).with_context(|| format!("Failed to read directory: {:?}", dir))? {
    let entry_dir = entry_dir?;
    let path = entry_dir.path();

    if !path.is_dir() {
      continue;
    }

    let base_dir = match path.file_name() {
      Some(name) => name.to_string_lossy(),
      None => continue,
    };
    if !base_dir.starts_with("main_") {
      continue;
    }

    let exclude_list = get_all_files(&path)?;

    let mut current_group = FileGroup {
      files: Vec::new(),
      total_size: 0,
    };

    for entry_file in fs::read_dir(&path).with_context(|| format!("Failed to read directory: {:?}", path))? {
      let entry_file = entry_file?;
      let fpath = entry_file.path();

      // Получаем имя файла (не папки!)
      let file_name = match fpath.file_name() {
        Some(name) => name.to_string_lossy().to_string(),
        None => continue,
      };

      // Пропускаем, если файл в чёрном списке
      if exclude_list.contains(&file_name) {
        continue;
      }

      // Проверяем префикс
      if !file_name.starts_with("game.7z") {
        continue;
      }

      current_group.files.push(fpath);
    }

    groups.push(current_group);
  }

  Ok(groups)
}
