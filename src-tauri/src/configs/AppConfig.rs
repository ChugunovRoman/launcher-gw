use crate::logger::LogLevel;

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;
use tauri::path::BaseDirectory;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunParams {
  #[serde(default)]
  pub cmd_params: Vec<String>,
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
  pub vid_mode: String,
}

impl Default for RunParams {
  fn default() -> Self {
    Self {
      cmd_params: Vec::new(),
      check_spawner: false,
      check_wait_press_any_key: true,
      check_without_cache: false,
      check_vsync: true,
      check_no_staging: true,
      vid_mode: detect_primary_monitor_mode().unwrap_or_else(|| "1920x1080 (60Hz)".to_string()),
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
  #[serde(default)]
  pub first_run: bool,
  #[serde(default)]
  pub install_path: String,
  #[serde(default)]
  pub client_uuid: String,
  #[serde(default)]
  pub log_level: LogLevel,
  #[serde(default)]
  pub run_params: RunParams,
}

impl Default for AppConfig {
  fn default() -> Self {
    let install_path = Self::get_path();

    Self {
      first_run: true,
      install_path,
      client_uuid: Uuid::new_v4().to_string(),
      log_level: LogLevel::Info,
      run_params: RunParams::default(),
    }
  }
}

impl AppConfig {
  /// Загружает конфиг из файла. Если файла нет — создаёт новый с first_run = true.
  pub fn load_or_create(app_handle: &tauri::AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
    let config_dir = app_handle
      .path()
      .resolve("config.json", BaseDirectory::AppConfig)
      .map(|p| p.parent().unwrap().to_path_buf())?;

    fs::create_dir_all(&config_dir)?;

    let config_path = config_dir.join("config.json");

    let config = if config_path.exists() {
      let content = fs::read_to_string(&config_path)?;
      let mut config: AppConfig = serde_json::from_str(&content)?;
      config.first_run = false; // если файл есть — это не первый запуск
      config.install_path = Self::get_path();
      config.save(&config_path)?;
      config
    } else {
      let mut config = AppConfig::default();
      config.first_run = true;
      config.install_path = Self::get_path();
      config.save(&config_path)?;
      config
    };

    Ok(config)
  }

  fn get_path() -> String {
    std::env::current_exe()
      .map(|exe| {
        exe
          .parent()
          .map(|p| p.to_path_buf())
          .unwrap_or_else(|| PathBuf::from("."))
      })
      .unwrap_or_else(|_| PathBuf::from("."))
      .to_string_lossy()
      .into_owned()
  }

  /// Перезагружает конфиг из файла (актуализирует текущую структуру)
  pub fn reload(
    &mut self,
    app_handle: &tauri::AppHandle,
  ) -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = app_handle
      .path()
      .resolve("config.json", BaseDirectory::AppConfig)
      .map(|p| p.parent().unwrap().to_path_buf())?;

    let config_path = config_dir.join("config.json");
    if config_path.exists() {
      let content = fs::read_to_string(&config_path)?;
      *self = serde_json::from_str(&content)?;
    }
    Ok(())
  }

  /// Сохраняет текущее состояние конфига в файл
  pub fn save(&self, config_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(self)?;
    fs::write(config_path, json)?;
    Ok(())
  }

  /// Сохраняет конфиг в стандартное место (удобно для вызова из команд)
  pub fn save_to_default(
    &self,
    app_handle: &tauri::AppHandle,
  ) -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = app_handle
      .path()
      .resolve("config.json", BaseDirectory::AppConfig)
      .map(|p| p.parent().unwrap().to_path_buf())?;

    let config_path = config_dir.join("config.json");
    self.save(&config_path)
  }
}

/// Определяет разрешение и частоту основного монитора
fn detect_primary_monitor_mode() -> Option<String> {
  // Используем winit для получения информации о мониторе
  // Это работает, потому что Tauri использует winit под капотом
  use winit::event_loop::EventLoop;
  let event_loop = EventLoop::new().ok()?;
  let primary = event_loop.primary_monitor()?;
  let mode = primary.video_modes().next()?;
  let size = mode.size();
  let refresh_rate = mode.refresh_rate_millihertz() / 1000; // Hz
  Some(format!(
    "{}x{} ({}Hz)",
    size.width, size.height, refresh_rate
  ))
}
