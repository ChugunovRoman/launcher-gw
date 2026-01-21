use std::{
  path::{Path, PathBuf},
  sync::Arc,
};

use tokio::sync::Mutex;

use crate::{
  configs::{AppConfig::AppConfig, GameConfig::GameConfig},
  service::{dto::ProfileItem, keybind_manager::KeybindManager},
};

#[tauri::command]
pub async fn add_profile(keybind_manager: tauri::State<'_, Arc<KeybindManager>>, name: String, basedOnProfile: String) -> Result<(), String> {
  log::debug!("add_profile, name: {}, basedOnProfile: {}", &name, &basedOnProfile);

  keybind_manager
    .create_profile_from(&name, &basedOnProfile)
    .await
    .map_err(|e| e.to_string())?;

  Ok(())
}

#[tauri::command]
pub async fn delete_profile(keybind_manager: tauri::State<'_, Arc<KeybindManager>>, name: String) -> Result<(), String> {
  log::debug!("delete_profile, name: {}", &name);

  keybind_manager.delete_profile(&name).await.map_err(|e| e.to_string())?;

  Ok(())
}

#[tauri::command]
pub async fn rename_profile(keybind_manager: tauri::State<'_, Arc<KeybindManager>>, oldName: String, newName: String) -> Result<(), String> {
  log::debug!("rename_profile, oldName: {}, newName: {}", &oldName, &newName);
  keybind_manager.rename_profile(&oldName, &newName).await.map_err(|e| e.to_string())?;

  Ok(())
}

#[tauri::command]
pub async fn set_apply_profile(app_config: tauri::State<'_, Arc<Mutex<AppConfig>>>, profileName: String, apply: bool) -> Result<(), String> {
  log::debug!("set_apply_profile, profileName: {}, apply: {}", &profileName, &apply);
  {
    let mut cfg_guard = app_config.lock().await;
    cfg_guard.selected_profile = if apply { Some(profileName) } else { None };
    cfg_guard.save().map_err(|e| e.to_string())?;
  };

  Ok(())
}

#[tauri::command]
pub async fn save_key_profiles(keybind_manager: tauri::State<'_, Arc<KeybindManager>>, profiles: Vec<ProfileItem>) -> Result<(), String> {
  log::debug!("save_key_profiles: Saving {} profiles", profiles.len());

  keybind_manager.save_all_profiles(profiles).await.map_err(|e| e.to_string())?;

  Ok(())
}

#[tauri::command]
pub async fn save_single_profile(keybind_manager: tauri::State<'_, Arc<KeybindManager>>, profile: ProfileItem) -> Result<(), String> {
  keybind_manager.save_all_profiles(vec![profile]).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_profile(keybind_manager: tauri::State<'_, Arc<KeybindManager>>, name: String, path: String) -> Result<(), String> {
  let dest_path = PathBuf::from(path);
  keybind_manager.export_profile(&name, dest_path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_profile(keybind_manager: tauri::State<'_, Arc<KeybindManager>>, path: String) -> Result<ProfileItem, String> {
  let src_path = PathBuf::from(path);
  keybind_manager.import_profile(src_path).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn apply_profile_to_ltx(
  keybind_manager: tauri::State<'_, Arc<KeybindManager>>,
  profileName: String,
  ltxPath: String,
) -> Result<(), String> {
  // 1. Проверяем существование ltx файла
  let path = Path::new(&ltxPath);
  if !path.exists() {
    return Err(format!("Файл не найден по пути: {}", ltxPath));
  }

  // 2. Ищем профиль в менеджере
  let profiles = keybind_manager.get_profiles().await;
  let profile_config = profiles
    .get(&profileName)
    .ok_or_else(|| format!("Профиль с именем '{}' не найден", profileName))?;

  // 3. Создаем новый экземпляр GameConfig для целевого файла
  let mut target_config = GameConfig::new(&ltxPath);

  // 4. Загружаем данные из целевого ltx
  target_config.load().map_err(|e| format!("Ошибка загрузки целевого конфига: {}", e))?;

  // 5. Вызываем метод merge
  // Передаем GameConfig из профиля
  target_config.merge(profile_config);

  // 6. Сохраняем изменения обратно в файл
  target_config.save().map_err(|e| format!("Ошибка сохранения конфига: {}", e))?;

  Ok(())
}
