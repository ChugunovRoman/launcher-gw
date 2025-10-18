use crate::logger::LogLevel;
use crate::utils::video::get_available_resolutions;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::Manager;
use tauri::path::BaseDirectory;
use uuid::Uuid;

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
  pub vid_modes: Vec<String>,
  #[serde(default)]
  pub vid_mode_latest: String,
  #[serde(default)]
  pub log_level: LogLevel,
  #[serde(default)]
  pub run_params: RunParams,

  pub path: String,
}

impl Default for AppConfig {
  fn default() -> Self {
    let install_path = Self::get_path();
    let modes = get_available_resolutions()
      .ok()
      .unwrap_or_else(|| vec!["800x600 (60Hz)".to_string()]);
    let max_mode = modes[0].clone();

    let mut run_params = RunParams::default();

    run_params.vid_mode = max_mode.clone();

    Self {
      first_run: true,
      install_path,
      client_uuid: Uuid::new_v4().to_string(),
      vid_modes: modes,
      vid_mode_latest: max_mode,
      log_level: LogLevel::Info,
      run_params: run_params,
      path: "".to_string(),
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
    let path = config_path
      .clone()
      .into_os_string()
      .to_str()
      .unwrap()
      .to_string();

    let config = if config_path.exists() {
      let content = fs::read_to_string(&config_path)?;
      let mut json_value: Value = serde_json::from_str(&content)?;
      let mut default = AppConfig::default();
      let default_value: Value = serde_json::to_value(default.clone())?;

      // Извлекаем мапы
      let loaded_map = match &mut json_value {
        Value::Object(m) => m,
        _ => return Err("config.json root must be an object".into()),
      };

      let default_map = match &default_value {
        Value::Object(m) => m,
        _ => unreachable!(),
      };

      // Мержим все поля, кроме run_params
      for (key, value) in default_map {
        if key == "run_params" {
          continue;
        }
        loaded_map
          .entry(key.clone())
          .or_insert_with(|| value.clone());
      }

      // Обработка run_params
      if let Some(default_run_params) = default_map.get("run_params") {
        let entry = loaded_map
          .entry("run_params".to_string())
          .or_insert_with(|| default_run_params.clone());

        match entry {
          Value::Object(loaded_rp) => {
            if let Value::Object(default_rp) = default_run_params {
              for (k, v) in default_rp {
                loaded_rp.entry(k.clone()).or_insert_with(|| v.clone());
              }
            }
          }
          _ => {
            // Заменяем повреждённый run_params на дефолтный
            *entry = default_run_params.clone();
          }
        }
      }

      let mut config = match serde_json::from_value::<AppConfig>(json_value.clone()) {
        Ok(cfg) => cfg,
        Err(e) => {
          log::error!(
            "⚠️ Failed to parse config.json (possibly outdated schema): {}",
            e
          );
          log::error!("   Replacing with default config.");
          // Возвращаем дефолтный конфиг и сохраняем его
          let preserved_uuid = if let Value::Object(map) = &json_value {
            match map.get("client_uuid") {
              Some(Value::String(uuid_str)) => uuid_str.clone(),
              _ => Uuid::new_v4().to_string(), // если нет или не строка — генерируем новый
            }
          } else {
            Uuid::new_v4().to_string()
          };

          default.first_run = false;
          default.client_uuid = preserved_uuid;
          default.install_path = Self::get_path();
          default.path = path.clone();
          // Перезаписываем файл
          default.save()?;
          default
        }
      };

      config.first_run = false;
      config.install_path = Self::get_path();
      config.path = path.clone();
      config.save()?; // сохраняем обновлённый файл
      config
    } else {
      let mut config = AppConfig::default();
      config.first_run = true;
      config.install_path = Self::get_path();
      config.path = path.clone();
      config.save()?;
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
  pub fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string_pretty(self)?;
    fs::write(&self.path, json)?;
    Ok(())
  }
}
