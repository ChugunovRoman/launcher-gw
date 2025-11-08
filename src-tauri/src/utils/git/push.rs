use anyhow::{Context, Result};
use git2::{Oid, Repository, Signature};
use std::{
  path::{Path, PathBuf},
  time::Duration,
};
use tokio::{
  io::{AsyncBufReadExt, BufReader},
  process::{Child, Command as TokioCommand},
};

use crate::utils::errors::upload_log;

#[derive(Debug)]
struct PushProgress {
  stage: String, // "Compressing", "Writing", "Counting", "Uploading"
  percent: f64,
  current: usize,
  total: usize,
  bytes: Option<usize>,
}

pub fn create_commit(repo: &Repository, files: &[PathBuf], base_dir: &Path, author_name: &str, author_email: &str) -> Result<Oid> {
  let mut index = repo.index()?;
  for file in files {
    let rel_path = file
      .strip_prefix(base_dir)
      .with_context(|| format!("File {:?} not under base dir {:?}", file, base_dir))?;
    index.add_path(rel_path)?;
  }
  let tree_oid = index.write_tree()?;
  index.write()?;

  let signature = Signature::now(author_name, author_email).with_context(|| "Failed to create git signature")?;

  let tree = repo.find_tree(tree_oid)?;

  // Проверяем, есть ли уже HEAD (т.е. есть ли хоть один коммит)
  let commit_oid = if let Ok(head) = repo.head() {
    // Уже есть коммиты — обычный коммит с родителем
    let parent = head.peel_to_commit()?;
    repo.commit(
      Some("HEAD"),
      &signature,
      &signature,
      &format!("Add {} files", files.len()),
      &tree,
      &[&parent],
    )?
  } else {
    // Первый коммит — корневой, без родителя
    let commit_oid = repo.commit(
      None, // временно без HEAD
      &signature,
      &signature,
      &format!("Add {} files", files.len()),
      &tree,
      &[], // нет родителей
    )?;

    // Теперь создаём ветку и устанавливаем HEAD
    let commit = repo.find_commit(commit_oid)?;
    let branch_name = "master";
    repo.branch(branch_name, &commit, false)?; // false = не форсировать
    repo.set_head(&format!("refs/heads/{}", branch_name))?;

    commit_oid
  };

  Ok(commit_oid)
}

/// Парсит строки вида:
/// "Compressing objects: 100% (123/123)"
/// "Writing objects:  45% (2250/5000), 12.34 MiB | 5.00 MiB/s"
/// "Uploading objects: 100% (5000/5000), 12.34 MiB"
fn parse_git_push_line(line: &str) -> Option<PushProgress> {
  if let Some(stripped) = line.strip_prefix("Compressing objects: ") {
    parse_percent_line("Compressing", stripped)
  } else if let Some(stripped) = line.strip_prefix("Writing objects: ") {
    parse_percent_line("Writing", stripped)
  } else if let Some(stripped) = line.strip_prefix("Counting objects: ") {
    parse_percent_line("Counting", stripped)
  } else if let Some(stripped) = line.strip_prefix("Uploading objects: ") {
    parse_percent_line("Uploading", stripped)
  } else {
    None
  }
}

fn parse_percent_line(stage: &str, input: &str) -> Option<PushProgress> {
  // Пример: "100% (123/123)" или " 45% (2250/5000), 12.34 MiB | ..."
  let percent_part = input.split_whitespace().next()?;
  if !percent_part.ends_with('%') {
    return None;
  }

  let percent_str = &percent_part[..percent_part.len() - 1];
  let percent = percent_str.parse::<f64>().ok()?;

  // Ищем (current/total)
  if let Some(start) = input.find('(') {
    if let Some(end) = input[start..].find(')') {
      let nums = &input[start + 1..start + end];
      let parts: Vec<&str> = nums.split('/').collect();
      if parts.len() == 2 {
        let current = parts[0].parse().ok()?;
        let total = parts[1].parse().ok()?;

        // Опционально: пытаемся вытащить байты (необязательно)
        let bytes = None; // можно расширить при желании

        return Some(PushProgress {
          stage: stage.to_string(),
          percent,
          current,
          total,
          bytes,
        });
      }
    }
  }

  None
}

async fn handle_git_output_line(app: &tauri::AppHandle, line: String) {
  log::info!("git push output: {}", &line);

  if let Some(progress) = parse_git_push_line(&line) {
    let msg = format!("{}: {}/{} ({:.1}%)", progress.stage, progress.current, progress.total, progress.percent);
    upload_log(app, msg);
  } else if line.contains("remote:") {
    upload_log(app, line);
  } else {
    upload_log(app, line);
  }
}
pub async fn push_head_with_retry(app: &tauri::AppHandle, repo_path: &PathBuf, max_retries: u32) -> Result<()> {
  for attempt in 0..=max_retries {
    let msg = format!("Push attempt {}", attempt + 1);
    upload_log(app, msg);

    // Запускаем git push --progress
    let mut command = TokioCommand::new("git");
    command
      .env("GIT_TERMINAL_PROMPT", "0") // отключаем интерактивный ввод
      .current_dir(repo_path)
      .args(["push", "--progress", "--verbose", "origin", "master"])
      .stderr(std::process::Stdio::piped());

    #[cfg(target_os = "windows")]
    {
      use std::os::windows::process::CommandExt;
      const CREATE_NO_WINDOW: u32 = 0x08000000;
      command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child: Child = command.spawn().with_context(|| "Failed to spawn git push")?;

    // --- Чтение stdout ---
    let stdout = child.stdout.take();
    let stdout_handle = if let Some(stdout) = stdout {
      let app = app.clone();
      Some(tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
          handle_git_output_line(&app, line).await;
        }
      }))
    } else {
      None
    };

    // --- Чтение stderr ---
    let stderr = child.stderr.take();
    let stderr_handle = if let Some(stderr) = stderr {
      let app = app.clone();
      Some(tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
          handle_git_output_line(&app, line).await;
        }
      }))
    } else {
      None
    };

    // --- Ожидание завершения процесса ---
    let status = child.wait().await.with_context(|| "git push process failed")?;

    // Дожидаемся завершения чтения потоков
    if let Some(handle) = stdout_handle {
      let _ = handle.await;
    }
    if let Some(handle) = stderr_handle {
      let _ = handle.await;
    }

    if status.success() {
      upload_log(app, "Push completed successfully".to_string());
      return Ok(());
    } else {
      let exit_code = status.code().unwrap_or(-1);
      let msg = format!("Push failed (attempt {}), exit code: {}", attempt + 1, exit_code);
      upload_log(app, msg);

      if attempt < max_retries {
        tokio::time::sleep(Duration::from_secs(2u64.pow(attempt))).await;
      } else {
        return Err(anyhow::anyhow!(
          "Push failed after {} attempts, last exit code: {}",
          max_retries + 1,
          exit_code
        ));
      }
    }
  }

  unreachable!()
}
