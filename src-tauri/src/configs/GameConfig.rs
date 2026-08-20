use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::consts::NO_KEY;

#[derive(Debug, Clone)]
pub struct GameConfig {
  data: HashMap<String, HashMap<String, String>>,
  file_path: String,
  /// Original file lines kept so patch saves do not wipe unrelated cvars/comments.
  raw_lines: Vec<String>,
  /// Keys changed since last load: (section_or_cvar, nested_or_same).
  dirty: HashSet<(String, String)>,
  /// Keys removed since last load (profile NO_KEY).
  removed: HashSet<(String, String)>,
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
      raw_lines: Vec::new(),
      dirty: HashSet::new(),
      removed: HashSet::new(),
    }
  }

  /// Загрузить из файла. Missing file = empty config (Ok).
  pub fn load(&mut self) -> Result<()> {
    if self.file_path.is_empty() {
      bail!("load() user.ltx read error ! file_path is not set ! Empty string !")
    }

    self.data.clear();
    self.dirty.clear();
    self.removed.clear();
    self.raw_lines.clear();

    if !Path::new(&self.file_path).exists() {
      return Ok(());
    }

    let content = fs::read_to_string(&self.file_path).with_context(|| format!("Failed to read config file: {}", self.file_path))?;

    self.raw_lines = content.lines().map(|l| l.to_string()).collect();

    for line in &self.raw_lines {
      let line = line.trim();
      if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
        continue;
      }

      let spaces_count = line.matches(' ').count();

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
        // Bare command (e.g. default_controls) — kept via raw_lines, not in data.
      }
    }

    Ok(())
  }

  /// Patch-save: rewrite only dirty/removed keys; keep comments, bare cmds, untouched cvars.
  pub fn save(&self) -> Result<()> {
    if self.file_path.is_empty() {
      bail!("save() user.ltx read error ! file_path is not set ! Empty string !")
    }

    let source_lines: Vec<String> = if !self.raw_lines.is_empty() {
      self.raw_lines.clone()
    } else if Path::new(&self.file_path).exists() {
      fs::read_to_string(&self.file_path)
        .with_context(|| format!("Failed to read config file for patch save: {}", self.file_path))?
        .lines()
        .map(|l| l.to_string())
        .collect()
    } else {
      Vec::new()
    };

    // Full rewrite fallback only when caller filled `data` without load/dirty
    // (legacy callers). Prefer dirty patch when dirty/removed are set OR raw exists.
    let use_patch = !self.dirty.is_empty() || !self.removed.is_empty() || !source_lines.is_empty();

    if !use_patch {
      return self.save_full_from_data();
    }

    let mut dirty_line: HashMap<(String, String), String> = HashMap::new();
    for (key1, key2) in &self.dirty {
      if let Some(inner) = self.data.get(key1) {
        if let Some(value) = inner.get(key2) {
          let line = if key1 == key2 {
            format!("{} {}", key1, value)
          } else {
            format!("{} {} {}", key1, key2, value)
          };
          dirty_line.insert((key1.clone(), key2.clone()), line);
        }
      }
    }

    let mut result: Vec<String> = Vec::new();
    let mut emitted: HashSet<(String, String)> = HashSet::new();

    for line in source_lines {
      let trimmed = line.trim();
      if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
        result.push(line);
        continue;
      }

      match parse_key_pair(trimmed) {
        Some(pair) if self.removed.contains(&pair) => {
          // drop removed binding/cvar
        }
        Some(pair) => {
          if let Some(new_line) = dirty_line.get(&pair) {
            result.push(new_line.clone());
            emitted.insert(pair);
          } else {
            result.push(line);
          }
        }
        None => result.push(line),
      }
    }

    for (pair, new_line) in &dirty_line {
      if !emitted.contains(pair) {
        result.push(new_line.clone());
      }
    }

    let mut out = result.join("\n");
    if !out.is_empty() && !out.ends_with('\n') {
      out.push('\n');
    }

    // Atomic tmp+rename so a crash mid-save cannot truncate user.ltx.
    crate::configs::atomic_write(&self.file_path, &out).with_context(|| format!("Failed to write config file: {}", self.file_path))?;

    Ok(())
  }

  fn save_full_from_data(&self) -> Result<()> {
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

    lines.sort();
    let mut out = lines.join("\n");
    if !out.is_empty() {
      out.push('\n');
    }
    // Atomic tmp+rename so a crash mid-save cannot truncate user.ltx.
    crate::configs::atomic_write(&self.file_path, &out).with_context(|| format!("Failed to write config file: {}", self.file_path))?;
    Ok(())
  }

  /// Получить значение по ключу
  pub fn get(&self, key: &str) -> Option<&HashMap<String, String>> {
    self.data.get(key)
  }

  /// Установить или обновить значение
  pub fn set(&mut self, key: String, value: String) {
    let mut map = HashMap::new();
    map.insert(key.clone(), value);
    self.data.insert(key.clone(), map);
    self.dirty.insert((key.clone(), key.clone()));
    self.removed.remove(&(key.clone(), key));
  }

  pub fn set2(&mut self, key: String, key2: String, value: String) {
    match self.data.get_mut(&key) {
      Some(found) => {
        found.insert(key2.clone(), value);
      }
      None => {
        let mut map = HashMap::new();
        map.insert(key2.clone(), value);
        self.data.insert(key.clone(), map);
      }
    };
    self.dirty.insert((key.clone(), key2.clone()));
    self.removed.remove(&(key, key2));
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
          if let Some(current_inner_map) = self.data.get_mut(other_key) {
            current_inner_map.remove(inner_key);
          }
          self.removed.insert((other_key.clone(), inner_key.clone()));
          self.dirty.remove(&(other_key.clone(), inner_key.clone()));
        } else {
          self
            .data
            .entry(other_key.clone())
            .or_insert_with(HashMap::new)
            .insert(inner_key.clone(), inner_value.clone());
          self.dirty.insert((other_key.clone(), inner_key.clone()));
          self.removed.remove(&(other_key.clone(), inner_key.clone()));
        }
      }
    }

    self.data.retain(|_, inner_map| !inner_map.is_empty());
  }
}

/// Parse a user.ltx line into (key1, key2) using the same rules as load().
fn parse_key_pair(line: &str) -> Option<(String, String)> {
  let spaces_count = line.matches(' ').count();
  let pos = line.find(' ')?;
  let key = &line[..pos];
  let value = line[pos + 1..].trim_start();

  if spaces_count == 2 {
    if let Some(pos2) = value.find(' ') {
      let key2 = &value[..pos2];
      return Some((key.to_string(), key2.to_string()));
    }
  }

  Some((key.to_string(), key.to_string()))
}
