use crate::consts::{BASE_DIR, CONFIG_NAME, VERSIONS_DIR};
use crate::handlers::dto::ReleaseManifest;
use crate::logger::LogLevel;
use crate::utils::video::get_available_resolutions;

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;
use tauri::path::BaseDirectory;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
  #[serde(default)]
  pub id: u32,
  #[serde(default)]
  pub name: String,
  #[serde(default)]
  pub path: String,
  #[serde(default)]
  pub installed_path: String,
  #[serde(default)]
  pub download_path: String,
  #[serde(default)]
  pub installed_updates: Vec<String>,
  #[serde(default)]
  pub is_local: bool,

  pub manifest: Option<ReleaseManifest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionProgress {
  #[serde(default)]
  pub id: u32,
  #[serde(default)]
  pub name: String,
  #[serde(default)]
  pub path: String,
  #[serde(default)]
  pub installed_path: String,
  #[serde(default)]
  pub download_path: String,
  #[serde(default)]
  pub files: HashMap<String, FileProgress>,
  #[serde(default)]
  pub is_downloaded: bool,
  #[serde(default)]
  pub downloaded_files_cnt: u16,
  #[serde(default)]
  pub total_file_count: u16,

  pub manifest: Option<ReleaseManifest>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionProgressUpload {
  #[serde(default)]
  pub name: String,
  #[serde(default)]
  pub path_dir: String,
  #[serde(default)]
  pub path_repo: String,
  #[serde(default)]
  pub files_per_commit: usize,
  #[serde(default)]
  pub total_groups: usize,
  #[serde(default)]
  pub uploaded_groups: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileProgress {
  #[serde(default)]
  pub id: String,
  #[serde(default)]
  pub project_id: u32,
  #[serde(default)]
  pub name: String,
  #[serde(default)]
  pub path: String,
  #[serde(default)]
  pub is_downloaded: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum LangType {
  Rus = 0,
  Eng,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum RenderType {
  RendererR2 = 0,
  RendererR25,
  RendererR3,
  RendererR4,
  RendererRgl,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunParams {
  #[serde(default)]
  pub cmd_params: String,
  #[serde(default)]
  pub check_spawner: bool,
  #[serde(default)]
  pub check_wait_press_any_key: bool,
  #[serde(default)]
  pub check_without_cache: bool,
  #[serde(default)]
  pub check_vsync: bool,
  #[serde(default)]
  pub check_no_staging: bool,
  #[serde(default)]
  pub windowed_mode: bool,
  #[serde(default)]
  pub ui_debug: bool,
  #[serde(default)]
  pub checks: bool,
  #[serde(default)]
  pub debug_spawn: bool,
  #[serde(default)]
  pub vid_mode: String,
  pub render: RenderType,
  pub lang: LangType,
  #[serde(default)]
  pub fov: f64,
  #[serde(default)]
  pub hud_fov: f64,
}

impl Default for RunParams {
  fn default() -> Self {
    Self {
      cmd_params: "".to_string(),
      check_spawner: false,
      check_wait_press_any_key: true,
      check_without_cache: false,
      check_vsync: true,
      check_no_staging: false,
      windowed_mode: true,
      ui_debug: false,
      checks: false,
      debug_spawn: false,
      vid_mode: "800x600 (60Hz)".to_string(),
      render: RenderType::RendererR4,
      lang: LangType::Rus,
      fov: 82.0,
      hud_fov: 0.6,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
  #[serde(default)]
  pub latest_pid: i64,
  #[serde(default)]
  pub first_run: bool,
  #[serde(default)]
  pub install_path: String,
  #[serde(default)]
  pub default_installed_path: String,
  #[serde(default)]
  pub default_download_path: String,
  #[serde(default)]
  pub client_uuid: String,
  #[serde(default)]
  pub vid_modes: Vec<String>,
  #[serde(default)]
  pub vid_mode_latest: String,
  #[serde(default)]
  pub log_level: LogLevel,
  #[serde(default)]
  pub lang: String,
  #[serde(default)]
  pub run_params: RunParams,

  #[serde(default)]
  pub selected_version: Option<String>,
  #[serde(default)]
  pub installed_versions: HashMap<String, Version>,
  #[serde(default)]
  pub progress_download: HashMap<String, VersionProgress>,
  #[serde(default)]
  pub progress_upload: Option<VersionProgressUpload>,

  #[serde(default)]
  pub pack_source_dir: String,
  #[serde(default)]
  pub pack_target_dir: String,
  #[serde(default)]
  pub unpack_source_dir: String,
  #[serde(default)]
  pub unpack_target_dir: String,

  #[serde(default)]
  pub versions: Vec<Version>,

  #[serde(default)]
  pub choosed_version_path: Option<String>,

  #[serde(default)]
  pub tokens: HashMap<String, String>,

  // SKIPED PROPS
  #[serde(skip)]
  pub path: String,
}

impl Default for AppConfig {
  fn default() -> Self {
    let install_path = Self::get_path();
    let modes = get_available_resolutions().unwrap_or_else(|_| vec!["800x600 (60Hz)".to_string()]);
    let max_mode = modes.first().cloned().unwrap_or_else(|| "800x600 (60Hz)".to_string());

    let mut run_params = RunParams::default();
    run_params.vid_mode = max_mode.clone();

    Self {
      latest_pid: -1,
      first_run: true,
      install_path: install_path.clone(),
      default_installed_path: Path::new(&install_path).join(VERSIONS_DIR).to_string_lossy().to_string(),
      default_download_path: Path::new(&install_path).join(VERSIONS_DIR).to_string_lossy().to_string(),
      client_uuid: Uuid::new_v4().to_string(),
      vid_modes: modes,
      vid_mode_latest: max_mode,
      log_level: LogLevel::Debug,
      lang: "ru".to_string(),
      run_params,
      path: "".to_string(),
      pack_source_dir: "".to_string(),
      pack_target_dir: "".to_string(),
      unpack_source_dir: "".to_string(),
      unpack_target_dir: "".to_string(),
      installed_versions: HashMap::new(),
      selected_version: None,
      versions: vec![],
      progress_download: HashMap::new(),
      tokens: HashMap::new(),
      progress_upload: None,
      choosed_version_path: None,
    }
  }
}

impl AppConfig {
  /// Загружает конфиг из файла. Если файла нет — создаёт новый с first_run = true.
  pub fn load_or_create(app_handle: &tauri::AppHandle) -> Result<Self> {
    let config_dir = app_handle
      .path()
      .resolve(BASE_DIR, BaseDirectory::AppConfig)
      .context("Failed to resolve config directory")?
      .parent()
      .unwrap()
      .to_path_buf();

    fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

    let config_path = config_dir.join(CONFIG_NAME);
    let path = config_path
      .to_str()
      .ok_or_else(|| anyhow::anyhow!("Invalid UTF-8 in config path"))?
      .to_string();

    if !config_path.exists() {
      let mut config = AppConfig::default();
      config.first_run = true;
      config.install_path = Self::get_path();
      config.path = path;
      config.save()?;
      return Ok(config);
    }

    let content = match fs::read_to_string(&config_path) {
      Ok(c) => c,
      Err(e) => {
        log::warn!("Cannot read {} file, return default config, error: {:?}", CONFIG_NAME, e);
        let mut config = AppConfig::default();
        config.first_run = true;
        config.install_path = Self::get_path();
        config.path = path;
        config.save()?;
        return Ok(config);
      }
    };
    let mut json_value: Value = match serde_json::from_str(&content) {
      Ok(c) => c,
      Err(e) => {
        log::warn!("Cannot parse JSON from {} file, return default config, error: {:?}", CONFIG_NAME, e);
        let mut config = AppConfig::default();
        config.first_run = true;
        config.install_path = Self::get_path();
        config.path = path;
        config.save()?;
        return Ok(config);
      }
    };

    let mut default = AppConfig::default();
    let default_value: Value = match serde_json::to_value(&default) {
      Ok(c) => c,
      Err(e) => {
        log::warn!("Failed to serialize config {} file, return default config, error: {:?}", CONFIG_NAME, e);
        let mut config = AppConfig::default();
        config.first_run = true;
        config.install_path = Self::get_path();
        config.path = path;
        config.save()?;
        return Ok(config);
      }
    };

    let loaded_map = match &mut json_value {
      Value::Object(m) => m,
      _ => bail!("config.json root must be a JSON object"),
    };

    let default_map = match &default_value {
      Value::Object(m) => m,
      _ => unreachable!(),
    };

    // Merge all fields except run_params
    for (key, value) in default_map {
      if key == "run_params" {
        continue;
      }
      loaded_map.entry(key.clone()).or_insert_with(|| value.clone());
    }

    // Handle run_params merge
    if let Some(default_run_params) = default_map.get("run_params") {
      let entry = loaded_map.entry("run_params".to_string()).or_insert_with(|| default_run_params.clone());

      match entry {
        Value::Object(loaded_rp) => {
          if let Value::Object(default_rp) = default_run_params {
            for (k, v) in default_rp {
              loaded_rp.entry(k.clone()).or_insert_with(|| v.clone());
            }
          }
        }
        _ => {
          *entry = default_run_params.clone();
        }
      }
    }

    let mut config = match serde_json::from_value::<AppConfig>(json_value.clone()) {
      Ok(cfg) => cfg,
      Err(e) => {
        log::error!("⚠️ Failed to parse config.json (possibly outdated schema): {}", e);
        log::error!("   Replacing with default config.");

        let preserved_uuid = if let Value::Object(map) = &json_value {
          match map.get("client_uuid") {
            Some(Value::String(uuid_str)) => uuid_str.clone(),
            _ => Uuid::new_v4().to_string(),
          }
        } else {
          Uuid::new_v4().to_string()
        };

        default.first_run = false;
        default.client_uuid = preserved_uuid;
        default.install_path = Self::get_path();
        default.path = path.clone();
        default.save().context("Failed to save fallback config")?;
        default
      }
    };

    config.first_run = false;
    config.install_path = Self::get_path();
    config.path = path;
    config.save().context("Failed to save merged config")?;

    Ok(config)
  }

  fn get_path() -> String {
    std::env::current_exe()
      .map(|exe| exe.parent().map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from(".")))
      .unwrap_or_else(|_| PathBuf::from("."))
      .to_string_lossy()
      .into_owned()
  }

  /// Перезагружает конфиг из файла (актуализирует текущую структуру)
  pub fn reload(&mut self, app_handle: &tauri::AppHandle) -> Result<()> {
    let config_dir = app_handle
      .path()
      .resolve(CONFIG_NAME, BaseDirectory::AppConfig)
      .context("Failed to resolve config path")?
      .parent()
      .unwrap()
      .to_path_buf();

    let config_path = config_dir.join(CONFIG_NAME);
    if config_path.exists() {
      let content = fs::read_to_string(&config_path).context("Failed to read config.json during reload")?;
      *self = serde_json::from_str(&content).context("Failed to parse config.json during reload")?;
    }
    Ok(())
  }

  /// Сохраняет текущее состояние конфига в файл
  pub fn save(&self) -> Result<()> {
    ensure!(!self.path.is_empty(), "Config path is not set");
    let json = serde_json::to_string_pretty(self).context("Failed to serialize config to JSON")?;
    fs::write(&self.path, json).context("Failed to write config file")?;
    Ok(())
  }
}
