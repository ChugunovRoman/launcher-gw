use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Manager;
use tauri::path::BaseDirectory;
use tokio::sync::Mutex;

use crate::configs::GameConfig::GameConfig;
use crate::consts::*;
use crate::service::dto::{KeybindingMap, ProfileItem};

#[derive(Clone)]
pub struct KeybindManager {
  path: PathBuf,
  map: Arc<Mutex<HashMap<String, GameConfig>>>,
}

impl KeybindManager {
  pub fn new(app_handle: &tauri::AppHandle) -> Self {
    let path = Self::init_dir(app_handle).expect("KeybindManager:new, cannot init directory");

    Self {
      path,
      map: Arc::new(Mutex::new(HashMap::new())),
    }
  }

  pub async fn get_profile(&self, name: &str) -> Option<GameConfig> {
    let profiles = self.map.lock().await;
    profiles.get(name).cloned()
  }
  pub async fn get_profiles(&self) -> HashMap<String, GameConfig> {
    let profiles = self.map.lock().await;
    profiles.clone()
  }

  pub async fn get_profiles_str(&self) -> Vec<ProfileItem> {
    let profiles = self.map.lock().await;
    let mut arr: Vec<ProfileItem> = vec![];

    for (name, profile) in profiles.iter() {
      let mut item = ProfileItem {
        name: name.to_string(),
        keybinds: HashMap::new(),
      };

      for map in profile.get("bind").iter() {
        for (action, keys) in map.iter() {
          item.keybinds.insert(
            action.clone(),
            KeybindingMap {
              key: Some(keys.to_string()),
              altkey: None,
            },
          );
        }
      }
      for map in profile.get("bind_sec").iter() {
        for (action, keys) in map.iter() {
          if let Some(found) = item.keybinds.get_mut(action) {
            found.altkey = Some(keys.to_string());
          }
        }
      }

      arr.push(item);
    }

    arr
  }

  pub async fn rename_profile(&self, old_name: &str, new_name: &str) -> Result<()> {
    // Запрещаем пустые или недопустимые имена
    if new_name.is_empty() || new_name.contains('/') || new_name.contains('\\') {
      return Err(anyhow::anyhow!("Invalid profile name: {}", new_name));
    }

    let mut profiles = self.map.lock().await;

    // Проверяем, существует ли старый профиль
    if !profiles.contains_key(old_name) {
      return Err(anyhow::anyhow!("Profile '{}' does not exist", old_name));
    }

    // Проверяем, не занято ли новое имя
    if profiles.contains_key(new_name) {
      return Err(anyhow::anyhow!("Profile '{}' already exists", new_name));
    }

    // Получаем старый конфиг и его путь
    let mut config = profiles.remove(old_name).unwrap(); // безопасно, т.к. проверили выше
    let old_path = config.get_file_path();

    // Формируем новый путь
    let new_path = self.path.join(new_name);

    // Переименовываем файл на диске
    fs::rename(&old_path, &new_path).with_context(|| format!("Failed to rename profile file from {:?} to {:?}", old_path, new_path))?;

    // Обновляем путь в конфиге
    config.set_file_path(new_path);

    // Вставляем под новым ключом
    profiles.insert(new_name.to_string(), config);

    Ok(())
  }

  pub async fn create_profile_from(&self, new_name: &str, source_name: &str) -> Result<()> {
    // 1. Валидация имени нового профиля
    if new_name.is_empty() || new_name.contains('/') || new_name.contains('\\') {
      return Err(anyhow::anyhow!("Invalid profile name: {}", new_name));
    }

    let mut profiles = self.map.lock().await;

    // 2. Проверяем, существует ли исходный профиль
    let source_config = profiles
      .get(source_name)
      .ok_or_else(|| anyhow::anyhow!("Source profile '{}' does not exist", source_name))?;

    // 3. Проверяем, не занято ли новое имя
    if profiles.contains_key(new_name) {
      return Err(anyhow::anyhow!("Profile '{}' already exists", new_name));
    }

    // 4. Клонируем конфигурацию
    let mut new_config = source_config.clone();

    // 5. Формируем новый путь к файлу
    // Добавляем расширение .ltx, если вы используете его в load_profiles
    let mut file_name = new_name.to_string();
    if !file_name.ends_with(".ltx") {
      file_name.push_str(".ltx");
    }
    let new_path = self.path.join(file_name);

    // 6. Обновляем путь в новом конфиге и сохраняем на диск
    new_config.set_file_path(new_path);
    new_config
      .save()
      .with_context(|| format!("Failed to save new profile '{}' to disk", new_name))?;

    // 7. Добавляем в карту памяти
    profiles.insert(new_name.to_string(), new_config);

    Ok(())
  }

  pub async fn delete_profile(&self, name: &str) -> Result<()> {
    let mut profiles = self.map.lock().await;

    // 1. Проверяем, существует ли профиль в памяти
    let config = profiles.get(name).ok_or_else(|| anyhow::anyhow!("Profile '{}' not found", name))?;

    // 2. (Опционально) Защита от удаления дефолтных профилей
    // Если вы не хотите, чтобы удаляли DEFAULT_BIND_LTX
    if name == DEFAULT_BIND_LTX {
      return Err(anyhow::anyhow!("Cannot delete the default profile"));
    }

    // 3. Получаем путь к файлу перед удалением из карты
    let file_path = PathBuf::from(config.get_file_path());

    // 4. Удаляем файл с диска
    if file_path.exists() {
      fs::remove_file(&file_path).with_context(|| format!("Failed to delete profile file at {:?}", file_path))?;
    }

    // 5. Удаляем из HashMap
    profiles.remove(name);

    log::info!("Profile '{}' successfully deleted", name);

    Ok(())
  }

  pub async fn load_profiles(&self) -> Result<()> {
    for entry in std::fs::read_dir(&self.path)? {
      let entry = entry?;
      let path = entry.path();

      if !path.is_file() {
        continue;
      }
      if path.extension().and_then(|s| s.to_str()) != Some("ltx") {
        continue;
      }

      let name = entry.file_name().clone().into_string().expect("OsString was not valid UTF-8");

      log::debug!(
        "Load keybind profile, name: {} path: {:?} file_name: {:?} entry: {:?}",
        &name,
        &path,
        &entry.file_name(),
        &entry
      );

      let mut config = GameConfig::new(path);
      config.load()?;

      self.map.lock().await.insert(name, config);
    }

    if self.map.lock().await.len() == 0 {
      log::debug!("create_and_load_default_profile");
      self.create_and_load_default_profile().await?;
    }

    Ok(())
  }

  pub async fn save_all_profiles(&self, updated_profiles: Vec<ProfileItem>) -> Result<()> {
    let mut profiles = self.map.lock().await;

    for item in updated_profiles {
      // 1. Пропускаем сохранение, если это дефолтный профиль
      if item.name == DEFAULT_BIND_LTX {
        log::warn!("Attempted to save protected profile: {}. Skipping.", DEFAULT_BIND_LTX);
        continue;
      }

      // 2. Ищем существующий конфиг по имени в нашей карте
      if let Some(config) = profiles.get_mut(&item.name) {
        // Обновляем данные в объекте GameConfig
        for (action, binds) in item.keybinds {
          // Устанавливаем основную клавишу
          if let Some(k) = binds.key {
            config.set2("bind".to_string(), action.clone(), k);
          }

          // Устанавливаем альтернативную клавишу
          if let Some(ak) = binds.altkey {
            config.set2("bind_sec".to_string(), action, ak);
          }
        }

        // 3. Сохраняем обновленный конфиг на диск
        config.save().with_context(|| format!("Failed to save profile file: {}", item.name))?;

        log::debug!("Profile '{}' saved successfully", item.name);
      } else {
        log::error!("Profile '{}' not found in memory map during save", item.name);
      }
    }
    Ok(())
  }

  pub async fn create_and_load_default_profile(&self) -> Result<()> {
    let mut config = GameConfig::new(self.path.join(DEFAULT_BIND_LTX));

    config.set2("bind".to_owned(), "left".to_owned(), "kLEFT".to_owned());
    config.set2("bind".to_owned(), "right".to_owned(), "kRIGHT".to_owned());
    config.set2("bind".to_owned(), "up".to_owned(), "kUP".to_owned());
    config.set2("bind".to_owned(), "down".to_owned(), "kDOWN".to_owned());
    config.set2("bind".to_owned(), "forward".to_owned(), "kW".to_owned());
    config.set2("bind".to_owned(), "back".to_owned(), "kS".to_owned());
    config.set2("bind".to_owned(), "lstrafe".to_owned(), "kA".to_owned());
    config.set2("bind".to_owned(), "rstrafe".to_owned(), "kD".to_owned());
    config.set2("bind".to_owned(), "llookout".to_owned(), "kQ".to_owned());
    config.set2("bind".to_owned(), "rlookout".to_owned(), "kE".to_owned());
    config.set2("bind".to_owned(), "jump".to_owned(), "kSPACE".to_owned());
    config.set2("bind".to_owned(), "crouch".to_owned(), "kLCONTROL".to_owned());
    config.set2("bind".to_owned(), "accel".to_owned(), "kLSHIFT".to_owned());
    config.set2("bind".to_owned(), "sprint_toggle".to_owned(), "kX".to_owned());
    config.set2("bind".to_owned(), "cam_zoom_in".to_owned(), "kADD".to_owned());
    config.set2("bind".to_owned(), "cam_zoom_out".to_owned(), "kSUBTRACT".to_owned());
    config.set2("bind".to_owned(), "torch".to_owned(), "kL".to_owned());
    config.set2("bind".to_owned(), "night_vision".to_owned(), "kN".to_owned());
    config.set2("bind".to_owned(), "show_detector".to_owned(), "kO".to_owned());
    config.set2("bind".to_owned(), "wpn_1".to_owned(), "k1".to_owned());
    config.set2("bind".to_owned(), "wpn_2".to_owned(), "k2".to_owned());
    config.set2("bind".to_owned(), "wpn_3".to_owned(), "k3".to_owned());
    config.set2("bind".to_owned(), "wpn_4".to_owned(), "k4".to_owned());
    config.set2("bind".to_owned(), "wpn_5".to_owned(), "k5".to_owned());
    config.set2("bind".to_owned(), "wpn_6".to_owned(), "k6".to_owned());
    config.set2("bind".to_owned(), "artefact".to_owned(), "k7".to_owned());
    config.set2("bind".to_owned(), "wpn_next".to_owned(), "kY".to_owned());
    config.set2("bind".to_owned(), "wpn_fire".to_owned(), "mouse1".to_owned());
    config.set2("bind".to_owned(), "wpn_zoom".to_owned(), "mouse2".to_owned());
    config.set2("bind".to_owned(), "wpn_reload".to_owned(), "kR".to_owned());
    config.set2("bind".to_owned(), "wpn_func".to_owned(), "kV".to_owned());
    config.set2("bind".to_owned(), "wpn_firemode_prev".to_owned(), "k9".to_owned());
    config.set2("bind".to_owned(), "wpn_firemode_next".to_owned(), "k0".to_owned());
    config.set2("bind".to_owned(), "pause".to_owned(), "kPAUSE".to_owned());
    config.set2("bind".to_owned(), "drop".to_owned(), "kG".to_owned());
    config.set2("bind".to_owned(), "use".to_owned(), "kF".to_owned());
    config.set2("bind".to_owned(), "scores".to_owned(), "kTAB".to_owned());
    config.set2("bind".to_owned(), "screenshot".to_owned(), "kF12".to_owned());
    config.set2("bind".to_owned(), "enter".to_owned(), "kRETURN".to_owned());
    config.set2("bind".to_owned(), "quit".to_owned(), "kESCAPE".to_owned());
    config.set2("bind".to_owned(), "console".to_owned(), "kGRAVE".to_owned());
    config.set2("bind".to_owned(), "inventory".to_owned(), "kI".to_owned());
    config.set2("bind".to_owned(), "buy_menu".to_owned(), "kB".to_owned());
    config.set2("bind".to_owned(), "team_menu".to_owned(), "kU".to_owned());
    config.set2("bind".to_owned(), "active_jobs".to_owned(), "kP".to_owned());
    config.set2("bind".to_owned(), "map".to_owned(), "kM".to_owned());
    config.set2("bind".to_owned(), "contacts".to_owned(), "kH".to_owned());
    config.set2("bind".to_owned(), "speech_menu_0".to_owned(), "kC".to_owned());
    config.set2("bind".to_owned(), "speech_menu_1".to_owned(), "kZ".to_owned());
    config.set2("bind".to_owned(), "quick_use_1".to_owned(), "kF1".to_owned());
    config.set2("bind".to_owned(), "quick_use_2".to_owned(), "kF2".to_owned());
    config.set2("bind".to_owned(), "quick_use_3".to_owned(), "kF3".to_owned());
    config.set2("bind".to_owned(), "quick_use_4".to_owned(), "kF4".to_owned());
    config.set2("bind".to_owned(), "quick_save".to_owned(), "kF5".to_owned());
    config.set2("bind".to_owned(), "quick_load".to_owned(), "kF9".to_owned());
    config.set2("bind".to_owned(), "editor".to_owned(), "kF10".to_owned());
    config.set2("bind".to_owned(), "ui_move_left".to_owned(), "kA".to_owned());
    config.set2("bind".to_owned(), "ui_move_right".to_owned(), "kD".to_owned());
    config.set2("bind".to_owned(), "ui_move_up".to_owned(), "kW".to_owned());
    config.set2("bind".to_owned(), "ui_move_down".to_owned(), "kS".to_owned());
    config.set2("bind".to_owned(), "ui_accept".to_owned(), "kRETURN".to_owned());
    config.set2("bind".to_owned(), "ui_back".to_owned(), "kESCAPE".to_owned());
    config.set2("bind".to_owned(), "ui_tab_prev".to_owned(), "kQ".to_owned());
    config.set2("bind".to_owned(), "ui_tab_next".to_owned(), "kE".to_owned());
    config.set2("bind".to_owned(), "ui_button_1".to_owned(), "k1".to_owned());
    config.set2("bind".to_owned(), "ui_button_2".to_owned(), "k2".to_owned());
    config.set2("bind".to_owned(), "ui_button_3".to_owned(), "k3".to_owned());
    config.set2("bind".to_owned(), "ui_button_4".to_owned(), "k4".to_owned());
    config.set2("bind".to_owned(), "ui_button_5".to_owned(), "k5".to_owned());
    config.set2("bind".to_owned(), "ui_button_6".to_owned(), "k6".to_owned());
    config.set2("bind".to_owned(), "ui_button_7".to_owned(), "k7".to_owned());
    config.set2("bind".to_owned(), "ui_button_8".to_owned(), "k8".to_owned());
    config.set2("bind".to_owned(), "ui_button_9".to_owned(), "k9".to_owned());
    config.set2("bind".to_owned(), "ui_button_0".to_owned(), "k0".to_owned());
    config.set2("bind".to_owned(), "pda_map_zoom_in".to_owned(), "kZ".to_owned());
    config.set2("bind".to_owned(), "pda_map_zoom_out".to_owned(), "kC".to_owned());
    config.set2("bind".to_owned(), "pda_map_zoom_reset".to_owned(), "kX".to_owned());
    config.set2("bind".to_owned(), "pda_map_show_actor".to_owned(), "kR".to_owned());
    config.set2("bind".to_owned(), "pda_map_show_legend".to_owned(), "kV".to_owned());
    config.set2("bind".to_owned(), "pda_filter_toggle".to_owned(), "kB".to_owned());
    config.set2("bind".to_owned(), "talk_switch_to_trade".to_owned(), "kX".to_owned());
    config.set2("bind".to_owned(), "talk_log_scroll_up".to_owned(), "kQ".to_owned());
    config.set2("bind".to_owned(), "talk_log_scroll_down".to_owned(), "kE".to_owned());

    config.set2("bind_sec".to_owned(), "enter".to_owned(), "kNUMPADENTER".to_owned());
    config.set2("bind_sec".to_owned(), "ui_move_left".to_owned(), "kLEFT".to_owned());
    config.set2("bind_sec".to_owned(), "ui_move_right".to_owned(), "kRIGHT".to_owned());
    config.set2("bind_sec".to_owned(), "ui_move_up".to_owned(), "kUP".to_owned());
    config.set2("bind_sec".to_owned(), "ui_move_down".to_owned(), "kDOWN".to_owned());
    config.set2("bind_sec".to_owned(), "ui_accept".to_owned(), "kF".to_owned());
    config.set2("bind_sec".to_owned(), "ui_back".to_owned(), "kG".to_owned());
    config.set2("bind_sec".to_owned(), "ui_action_1".to_owned(), "kY".to_owned());
    config.set2("bind_sec".to_owned(), "ui_action_2".to_owned(), "kN".to_owned());
    config.set2("bind_sec".to_owned(), "pda_map_move_left".to_owned(), "kLEFT".to_owned());
    config.set2("bind_sec".to_owned(), "pda_map_move_right".to_owned(), "kRIGHT".to_owned());
    config.set2("bind_sec".to_owned(), "pda_map_move_up".to_owned(), "kUP".to_owned());
    config.set2("bind_sec".to_owned(), "pda_map_move_down".to_owned(), "kDOWN".to_owned());
    config.set2("bind_sec".to_owned(), "pda_map_zoom_in".to_owned(), "kADD".to_owned());
    config.set2("bind_sec".to_owned(), "pda_map_zoom_out".to_owned(), "kSUBTRACT".to_owned());
    config.set2("bind_sec".to_owned(), "pda_map_zoom_reset".to_owned(), "kNUMPAD0".to_owned());
    config.set2("bind_sec".to_owned(), "pda_map_show_actor".to_owned(), "kNUMPADCOMMA".to_owned());
    config.set2("bind_sec".to_owned(), "pda_map_show_legend".to_owned(), "kMULTIPLY".to_owned());
    config.set2("bind_sec".to_owned(), "talk_log_scroll_up".to_owned(), "kPGUP".to_owned());
    config.set2("bind_sec".to_owned(), "talk_log_scroll_down".to_owned(), "kPGDN".to_owned());

    config.save()?;
    self.map.lock().await.insert(DEFAULT_BIND_LTX.to_string(), config.clone());

    config.set_file_path(self.path.join(CUSTOM_BIND_LTX));
    config.save()?;

    self.map.lock().await.insert(CUSTOM_BIND_LTX.to_string(), config);

    Ok(())
  }

  /// Экспорт: копирует файл профиля из внутренней папки в путь, выбранный пользователем
  pub async fn export_profile(&self, name: &str, destination_path: PathBuf) -> Result<()> {
    let profiles = self.map.lock().await;
    let config = profiles.get(name).ok_or_else(|| anyhow::anyhow!("Profile '{}' not found", name))?;

    let source_path = config.get_file_path();

    // Копируем файл
    fs::copy(&source_path, &destination_path).with_context(|| format!("Failed to copy profile from {:?} to {:?}", source_path, destination_path))?;

    Ok(())
  }

  /// Импорт: копирует внешний файл в папку профилей и загружает его в память
  pub async fn import_profile(&self, source_path: PathBuf) -> Result<ProfileItem> {
    let file_name = source_path
      .file_name()
      .and_then(|n| n.to_str())
      .ok_or_else(|| anyhow::anyhow!("Invalid source file name"))?;

    if !file_name.ends_with(".ltx") {
      return Err(anyhow::anyhow!("Only .ltx files are supported"));
    }

    let destination_path = self.path.join(file_name);

    // 1. Проверяем, не существует ли уже такой профиль
    if destination_path.exists() {
      return Err(anyhow::anyhow!("Profile with name '{}' already exists", file_name));
    }

    // 2. Копируем файл в папку профилей приложения
    fs::copy(&source_path, &destination_path)?;

    // 3. Загружаем новый конфиг в память
    let mut config = GameConfig::new(destination_path);
    config.load()?;

    let mut profiles = self.map.lock().await;
    profiles.insert(file_name.to_string(), config.clone());

    // 4. Возвращаем ProfileItem для обновления фронтенда
    // Здесь мы вручную собираем ProfileItem (логика аналогична get_profiles_str)
    let mut item = ProfileItem {
      name: file_name.to_string(),
      keybinds: std::collections::HashMap::new(),
    };

    // Заполняем бинды для возвращаемого объекта
    if let Some(map) = config.get("bind") {
      for (action, keys) in map.iter() {
        item.keybinds.insert(
          action.clone(),
          KeybindingMap {
            key: Some(keys.to_string()),
            altkey: None,
          },
        );
      }
    }
    if let Some(map) = config.get("bind_sec") {
      for (action, keys) in map.iter() {
        if let Some(found) = item.keybinds.get_mut(action) {
          found.altkey = Some(keys.to_string());
        }
      }
    }

    Ok(item)
  }

  fn init_dir(app_handle: &tauri::AppHandle) -> Result<PathBuf> {
    let config_dir = app_handle
      .path()
      .resolve(BASE_DIR, BaseDirectory::AppConfig)
      .context("Failed to resolve config directory")?
      .parent()
      .unwrap()
      .to_path_buf();

    fs::create_dir_all(&config_dir).context("Failed to create config directory")?;

    let profiles_dir = config_dir.join("profiles");

    fs::create_dir_all(&profiles_dir).context("Failed to create config directory")?;

    Ok(profiles_dir)
  }
}
