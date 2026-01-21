use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::consts::NO_KEY;

#[derive(Debug, Clone)]
pub struct GameConfig {
  data: HashMap<String, HashMap<String, String>>,
  file_path: String,
}

#[derive(Debug, Clone)]
pub struct UserLtx(pub GameConfig);

#[derive(Debug, Clone)]
pub struct TmpLtx(pub GameConfig);

impl GameConfig {
  /// Создать новый конфиг с указанием пути
  pub fn new<P: AsRef<Path>>(path: P) -> Self {
    Self {
      data: HashMap::new(),
      file_path: path.as_ref().to_string_lossy().into_owned(),
    }
  }

  /// Загрузить из файла
  pub fn load(&mut self) -> Result<()> {
    if &self.file_path == "" {
      bail!("load() user.ltx read error ! file_path is not set ! Empty string !")
    }

    let content = fs::read_to_string(&self.file_path).with_context(|| format!("Failed to read config file: {}", self.file_path))?;

    self.data.clear();

    for line in content.lines() {
      let line = line.trim();
      if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
        continue; // пропускаем пустые строки и комментарии
      }

      let spaces_count = line.matches(" ").count();

      if let Some(pos) = line.find(' ') {
        let key = &line[..pos];
        let value = line[pos + 1..].trim_start();
        let mut map: HashMap<String, String> = HashMap::new();

        if spaces_count == 2 {
          if let Some(pos2) = value.find(' ') {
            let key2 = &value[..pos2];
            let value2 = value[pos2 + 1..].trim_start();
            match self.data.get_mut(key) {
              Some(found) => {
                found.insert(key2.to_string(), value2.to_string());
                continue;
              }
              None => {
                map.insert(key2.to_string(), value2.to_string());
              }
            }
          }
        } else {
          map.insert(key.to_string(), value.to_string());
        }

        self.data.insert(key.to_string(), map);
      } else {
        // Строка без значения — сохраняем как ключ = ""
        self.data.insert(line.to_string(), HashMap::new());
      }
    }

    Ok(())
  }

  /// Сохранить в файл
  pub fn save(&self) -> Result<()> {
    if &self.file_path == "" {
      bail!("save() user.ltx read error ! file_path is not set ! Empty string !")
    }

    let mut lines: Vec<String> = vec![];

    for map1 in self.data.iter() {
      let key1 = map1.0;
      for map2 in map1.1.iter() {
        let key2 = map2.0;
        let value = map2.1;
        if key1 == key2 {
          lines.push(format!("{} {}", key1, value));
        } else {
          lines.push(format!("{} {} {}", key1, key2, value));
        }
      }
    }

    lines.sort(); // опционально: для стабильного вывода

    fs::write(&self.file_path, lines.join("\n")).with_context(|| format!("Failed to write config file: {}", self.file_path))?;

    Ok(())
  }

  /// Получить значение по ключу
  pub fn get(&self, key: &str) -> Option<&HashMap<String, String>> {
    self.data.get(key)
  }

  /// Установить или обновить значение
  pub fn set(&mut self, key: String, value: String) {
    let mut map = HashMap::new();
    map.insert(key.to_string(), value);
    self.data.insert(key, map);
  }
  pub fn set2(&mut self, key: String, key2: String, value: String) {
    match self.data.get_mut(&key) {
      Some(found) => {
        found.insert(key2, value);
      }
      None => {
        let mut map = HashMap::new();
        map.insert(key2.to_string(), value);
        self.data.insert(key, map);
      }
    };
  }

  /// Получить путь к файлу
  pub fn get_file_path(&self) -> &str {
    &self.file_path
  }

  pub fn set_file_path<P: AsRef<Path>>(&mut self, path: P) {
    self.file_path = path.as_ref().to_path_buf().to_string_lossy().to_string();
  }

  pub fn merge(&mut self, other: &GameConfig) {
    for (other_key, other_inner_map) in &other.data {
      for (inner_key, inner_value) in other_inner_map {
        if inner_value == NO_KEY {
          // Логика удаления
          if let Some(current_inner_map) = self.data.get_mut(other_key) {
            current_inner_map.remove(inner_key);
          }
        } else {
          // Логика вставки/обновления (как была раньше)
          self
            .data
            .entry(other_key.clone())
            .or_insert_with(HashMap::new)
            .insert(inner_key.clone(), inner_value.clone());
        }
      }
    }

    // Удаляем пустые секции, которые могли остаться после удаления ключей
    self.data.retain(|_, inner_map| !inner_map.is_empty());
  }
}
