use std::{
  collections::HashSet,
  path::{Path, PathBuf},
  sync::Arc,
};
use tauri::{Emitter, Manager};
use tokio::{fs, sync::Mutex};

use crate::{
  configs::AppConfig::AppConfig,
  service::{get_release::ServiceGetRelease, main::Service},
  utils::{
    errors::{log_full_error, upload_log},
    git::{
      grouping::get_existing_groups,
      push::{create_commit, push_head_with_retry},
      repo::get_repo,
      state::*,
    },
  },
};

#[tauri::command]
pub async fn continue_upload(app: tauri::AppHandle) -> Result<(), String> {
  let progress_upload_opt = {
    let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("AppConfig not initialized")?;
    let config_guard = state.lock().await;

    config_guard.progress_upload.clone()
  };

  let progress_upload = match progress_upload_opt {
    Some(value) => value,
    None => {
      upload_log(&app, "Progress upload state doesn't exist in AppConfig ! Just return".to_owned());
      return Ok(());
    }
  };

  upload_log(
    &app,
    format!(
      "Start continue_upload, release name: {} groups: {}/{} filesPerCommit: {}",
      &progress_upload.name, &progress_upload.uploaded_groups, &progress_upload.total_groups, &progress_upload.files_per_commit
    ),
  );

  let base_dir = Path::new(&progress_upload.path_dir);
  let groups = get_existing_groups(&base_dir).map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  upload_log(&app, format!("Get all files as groups, total groups: {}", &groups.len()));

  upload_log(&app, format!("Start upload groups. Groups count: {}", &groups.len()));

  let first_repo_name = "main_1".to_owned();
  let first_repo_path = base_dir.join(&first_repo_name);
  let state = match load_sync_state(Path::new(&first_repo_path)).map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })? {
    Some(data) => data,
    None => {
      let msg = format!("continue_upload() Cannot find sync state file in repo: {:?}", first_repo_path);
      log::error!("{}", &msg);

      return Err(msg);
    }
  };
  let _ = app.emit("upload-files-count", (&state.uploaded_files_count, &state.total_files_count));

  for (i, group) in groups.into_iter().enumerate() {
    let index = i + 1;
    let repo_name = format!("main_{}", &index);
    let repo_path = base_dir.join(&repo_name);

    upload_log(
      &app,
      format!(
        "continue_upload repo_name: {}, repo_path: {:?} group.files: {}",
        &repo_name,
        &repo_path,
        &group.files.len()
      ),
    );

    upload_log(&app, format!("Open local repo {:?}", &repo_path));

    // получаем состояние загрузки
    let repo_state = match load_sync_state(&repo_path).map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })? {
      Some(data) => data,
      None => {
        upload_log(
          &app,
          format!("Repo state file doen't exists for repo: {:?}. Skip this repo !", &repo_path),
        );

        continue;
      }
    };

    if repo_state.state == UploadState::Completed {
      upload_log(&app, format!("All files will be pushed for repo: {:?}. Skip this repo !", &repo_path));

      continue;
    }

    // получаем ранее созданый локальный репо
    let repo = get_repo(&repo_path).map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

    let has_unpushed = get_unpushed_commit(&repo_path).map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

    if let Some(commit) = has_unpushed {
      upload_log(
        &app,
        format!("Found not pushed commit in repo: {:?} ! Sha commit: {}", &repo_path, &commit.0),
      );

      upload_log(&app, format!("Start push commit {} ...", &commit.0));

      // пушим 1 коммит
      push_head_with_retry(&app, &repo_path, 3).await.map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

      let files_in_commit = commit.1.files.len();

      upload_log(
        &app,
        format!("Commit {} was pushed successful ! files_in_commit: {}", &commit.0, &files_in_commit),
      );

      // Помечаем что коммит был успешно запушен на удаленный гит сервер
      set_commit_push(&repo_path, commit.0.clone(), true).map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

      let (count, total) = add_uploaded_files_count(&first_repo_path, files_in_commit).map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

      upload_log(
        &app,
        format!("Send event upload-files-count with data, count :{} total: {} !", &count, &total),
      );

      let _ = app.emit("upload-files-count", (count, total));
    };

    // разбиваем файлы группы на чанки. 1 чанк = 1 коммит = 1 пуш и так покругу для всех чанков
    let chunks: Vec<Vec<PathBuf>> = group.files.chunks(progress_upload.files_per_commit).map(|c| c.to_vec()).collect();

    upload_log(&app, format!("Repo {}: count chunks: {}", &repo_name, &chunks.len()));

    for (chunk_idx, chunk) in chunks.into_iter().enumerate() {
      let files_in_chunk = chunk.len();
      let file_names: HashSet<String> = chunk
        .iter()
        .filter_map(|path| path.file_name().and_then(|os_str| os_str.to_str()).map(|s| s.to_owned()))
        .collect();
      let commit = CommitSyncState {
        files: file_names,
        was_pushed: false,
      };

      upload_log(&app, format!("Commit chunk {} files count: {}", &chunk_idx, &chunk.len()));

      // коммитим файлы чанка
      let oid = create_commit(&repo, &chunk, &repo_path, "ChugunovRoman", "Zebs-BMK@yandex.ru").map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

      // Сохранаем инфу о созданном коммите
      add_commit(&repo_path, oid.clone().to_string(), commit).map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

      upload_log(&app, format!("Start push chunk {} with size: {}", &chunk_idx, &chunk.len()));

      // пушим 1 коммит
      push_head_with_retry(&app, &repo_path, 3).await.map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

      upload_log(
        &app,
        format!(
          "Push chunk {} with size: {} files_in_chunk: {} COMPLETED !",
          &chunk_idx,
          &chunk.len(),
          &files_in_chunk
        ),
      );

      // Помечаем что коммит был успешно запушен на удаленный гит сервер
      set_commit_push(&repo_path, oid.clone().to_string(), true).map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;
      let (count, total) = add_uploaded_files_count(&first_repo_path, files_in_chunk).map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

      upload_log(
        &app,
        format!("Send event upload-files-count with data, count :{} total: {} !", &count, &total),
      );

      let _ = app.emit("upload-files-count", (count, total));

      upload_log(&app, format!("Repo {}: pushed chunk {}", &repo_name, chunk_idx));
    }

    // Сохраняем в конфиг прогресс загрузки
    {
      let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("AppConfig not initialized")?;
      let mut config_guard = state.lock().await;

      if let Some(ref mut progress) = config_guard.progress_upload {
        progress.uploaded_groups = index;
      };
    };

    set_completed_status(&repo_path).map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

    upload_log(&app, format!("Upload {} group index successful !", &index));
  }

  {
    let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("AppConfig not initialized")?;
    let mut config_guard = state.lock().await;

    config_guard.progress_upload = None;
    config_guard.save().map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;
  };

  {
    let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
    let service_guard = state.lock().await;

    service_guard.set_release_visibility(progress_upload.path_repo.clone(), true).await
  }
  .map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  upload_log(&app, "FULL Upload completed successful !".to_string());

  log::info!("Full upload of version {} finish successful !", &progress_upload.name);

  Ok(())
}
