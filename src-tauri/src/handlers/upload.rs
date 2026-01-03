use regex::Regex;
use std::{
  collections::{HashMap, HashSet},
  fs,
  path::{Path, PathBuf},
  sync::Arc,
};
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

use crate::{
  configs::AppConfig::{AppConfig, VersionProgressUpload},
  consts::MANIFEST_NAME,
  service::{get_release::ServiceGetRelease, main::Service},
  utils::{
    errors::{log_full_error, upload_log},
    git::{
      grouping::group_files_by_size,
      push::{create_commit, push_head_with_retry},
      repo::*,
      state::*,
    },
  },
};

#[tauri::command]
pub async fn upload_release(app: tauri::AppHandle, name: String, path: String, filesPerCommit: usize) -> Result<(), String> {
  let base_dir = Path::new(&path);
  let mut token = "".to_owned();
  let manifest = {
    let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
    let service_guard = state.lock().await;
    let api = service_guard.api_client.current_provider().map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

    token = api.get_token();

    api.get_manifest()
  }
  .map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  upload_log(
    &app,
    format!(
      "Start upload_release, max_size: {} path: {} filesPerCommit: {}",
      &manifest.max_size, &path, &filesPerCommit
    ),
  );

  let releases = {
    let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
    let service_guard = state.lock().await;

    service_guard.get_releases().await
  }
  .map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  let release = releases
    .iter()
    .find(|r| r.name == name)
    .expect(&format!("upload_release(), Release by name {} not found !", &name));

  upload_log(&app, format!("Found release: {} ({})", &release.name, &release.id));

  let main_repos = {
    let state = app.try_state::<Arc<Mutex<Service>>>().ok_or("Service not initialized")?;
    let service_guard = state.lock().await;
    let api = service_guard.api_client.current_provider().map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

    api.get_release_repos_by_name(&release.name).await
  }
  .map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  let mut groups = group_files_by_size(base_dir, manifest.max_size).map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  upload_log(&app, format!("All files splited on {} groups", &groups.len()));

  {
    let state = app.try_state::<Arc<Mutex<AppConfig>>>().ok_or("AppConfig not initialized")?;
    let mut config_guard = state.lock().await;

    config_guard.progress_upload = Some(VersionProgressUpload {
      name: name.clone(),
      path_dir: path.clone(),
      path_repo: release.path.clone(),
      files_per_commit: filesPerCommit.clone(),
      total_groups: groups.len().clone(),
      uploaded_groups: 0,
    });
    config_guard.save().map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;
  };

  upload_log(&app, format!("Start upload groups. Groups count: {}", &groups.len()));

  let mut total_repo_files: usize = 0;
  // Создаем папку под каждый репо, перемещаем туда файлы из своей группы
  // создаем .gitignore и инитим репо
  for (i, group) in groups.iter_mut().enumerate() {
    let index = i + 1;
    let repo_name = format!("main_{}", &index);
    let repo_path = base_dir.join(&repo_name);

    upload_log(
      &app,
      format!(
        "Prepare local repo: {}, repo_path: {:?} group.files: {} size: {}",
        &repo_name,
        &repo_path,
        &group.files.len(),
        &group.total_size
      ),
    );

    fs::create_dir_all(&repo_path).map_err(|e| e.to_string())?;

    // Создаём .gitignore с игнорированием файла состояния
    create_gitignore_with_state(&repo_path).map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;
    // Создаём файл `.gitattributes`
    create_gitattributes(&repo_path).map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

    upload_log(&app, "Renaming files...".to_owned());

    // Перемещаем файлы в репо
    for file in &mut group.files {
      let file_name = file.file_name().unwrap();
      let dest = repo_path.join(file_name);
      fs::rename(&*file, &dest).map_err(|e| e.to_string())?;
      total_repo_files += 1;
      *file = dest;
    }
    if index == 1 {
      let manifest_path = base_dir.join(MANIFEST_NAME);
      let manifest_dest_path = repo_path.join(MANIFEST_NAME);
      if manifest_path.exists() {
        fs::rename(&manifest_path, &manifest_dest_path).map_err(|e| e.to_string())?;
      }
    }
    upload_log(&app, "Renaming files is completed !".to_owned());

    let _ = app.emit("upload-files-count", (0, &total_repo_files));

    let project = main_repos
      .iter()
      .find(|p| p.name.ends_with(&format!("main_{}", &index)))
      .expect(&format!("Project by ends_with(main_{}) name not found ! Cannot upload files !", &index));
    let re = Regex::new(r"^git@([^:]+):(.+)$").unwrap();
    let remote_url = re.replace(&project.ssh_remote_url, format!("https://oauth2:{}@$1/$2", token)).to_string();

    init_repo_with_remote(&repo_path, &remote_url).map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

    upload_log(&app, format!("Init repo {:?} !", &repo_path));
  }

  for (i, group) in groups.into_iter().enumerate() {
    let index = i + 1;
    let repo_name = format!("main_{}", &index);
    let repo_path = base_dir.join(&repo_name);
    let first_repo_name = "main_1".to_owned();
    let first_repo_path = base_dir.join(&first_repo_name);

    upload_log(
      &app,
      format!(
        "upload_release repo_name: {}, repo_path: {:?} group.files: {} size: {}",
        &repo_name,
        &repo_path,
        &group.files.len(),
        &group.total_size
      ),
    );

    upload_log(&app, format!("Open local repo {:?}", &repo_path));

    init_sync_state(&repo_path, total_repo_files).map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

    // получаем ранее созданый локальный репо
    let repo = get_repo(&repo_path).map_err(|e| {
      log_full_error(&e);
      e.to_string()
    })?;

    // разбиваем файлы группы на чанки. 1 чанк = 1 коммит = 1 пуш и так покругу для всех чанков
    let chunks: Vec<Vec<PathBuf>> = group.files.chunks(filesPerCommit).map(|c| c.to_vec()).collect();

    upload_log(&app, format!("Repo {}: count chunks: {}", &repo_name, &chunks.len()));

    let gitignore_path = repo_path.join(".gitignore");
    let gitattributes_path = repo_path.join(".gitattributes");
    let manifest_path = repo_path.join(MANIFEST_NAME);

    for (chunk_idx, chunk) in chunks.into_iter().enumerate() {
      let mut current_chunk = chunk;
      let files_in_chunk = current_chunk.len();

      // Только в первой итерации добавляем .gitignore .gitattributes и manifest.json, если они существуют
      if chunk_idx == 0 {
        let mut extra_files = Vec::new();
        if gitignore_path.exists() {
          extra_files.push(gitignore_path.clone());
        }
        if gitattributes_path.exists() {
          extra_files.push(gitattributes_path.clone());
        }
        if manifest_path.exists() {
          extra_files.push(manifest_path.clone());
        }

        current_chunk.extend(extra_files);
      }
      let idx = chunk_idx + 1;

      let file_names: HashSet<String> = current_chunk
        .iter()
        .filter_map(|path| path.file_name().and_then(|os_str| os_str.to_str()).map(|s| s.to_owned()))
        .collect();
      let commit = CommitSyncState {
        files: file_names,
        was_pushed: false,
      };

      upload_log(&app, format!("Commit chunk {} files count: {}", &idx, &current_chunk.len()));

      // коммитим файлы чанка
      let oid = create_commit(&repo, &current_chunk, &repo_path, "ChugunovRoman", "Zebs-BMK@yandex.ru").map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

      // Сохранаем инфу о созданном коммите
      add_commit(&repo_path, oid.clone().to_string(), commit).map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

      upload_log(&app, format!("Start push chunk {} with size: {}", &idx, &current_chunk.len()));

      // пушим 1 коммит
      push_head_with_retry(&app, &repo_path, 3).await.map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

      upload_log(&app, format!("Push chunk {} with size: {} COMPLETED !", &idx, &current_chunk.len()));

      // Помечаем что коммит был успешно запушен на удаленный гит сервер
      set_commit_push(&repo_path, oid.clone().to_string(), true).map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;
      let (count, total) = add_uploaded_files_count(&first_repo_path, files_in_chunk).map_err(|e| {
        log_full_error(&e);
        e.to_string()
      })?;

      let _ = app.emit("upload-files-count", (count, total));

      upload_log(&app, format!("Repo {}: pushed chunk {}", &repo_name, idx));
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

  // Сбрасыаем процесс загрузки когда уже все полностью загружено
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

    service_guard.set_release_visibility(&release.name, true).await
  }
  .map_err(|e| {
    log_full_error(&e);
    e.to_string()
  })?;

  upload_log(&app, "FULL Upload completed successful !".to_string());

  log::info!("Full upload of version {} finish successful !", &name);

  Ok(())
}
