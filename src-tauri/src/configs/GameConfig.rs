use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct GameConfig {
  data: HashMap<String, String>,
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
    let content = fs::read_to_string(&self.file_path).with_context(|| format!("Failed to read config file: {}", self.file_path))?;

    self.data.clear();

    for line in content.lines() {
      let line = line.trim();
      if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
        continue; // пропускаем пустые строки и комментарии
      }

      if let Some(pos) = line.find(' ') {
        let key = &line[..pos];
        let value = line[pos + 1..].trim_start();
        self.data.insert(key.to_string(), value.to_string());
      } else {
        // Строка без значения — сохраняем как ключ = ""
        self.data.insert(line.to_string(), String::new());
      }
    }

    Ok(())
  }

  /// Сохранить в файл
  pub fn save(&self) -> Result<()> {
    let mut lines: Vec<String> = self.data.iter().map(|(k, v)| format!("{} {}", k, v)).collect();

    lines.sort(); // опционально: для стабильного вывода

    fs::write(&self.file_path, lines.join("\n")).with_context(|| format!("Failed to write config file: {}", self.file_path))?;

    Ok(())
  }

  /// Получить значение по ключу
  pub fn get(&self, key: &str) -> Option<&String> {
    self.data.get(key)
  }

  /// Установить или обновить значение
  pub fn set(&mut self, key: String, value: String) {
    self.data.insert(key, value);
  }

  /// Получить путь к файлу
  pub fn get_file_path(&self) -> &str {
    &self.file_path
  }

  pub fn set_file_path<P: AsRef<Path>>(&mut self, path: P) {
    self.file_path = path.as_ref().to_path_buf().to_string_lossy().to_string();
  }
}
