use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct FileGroup {
  files: Vec<PathBuf>,
  total_size: u64,
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
