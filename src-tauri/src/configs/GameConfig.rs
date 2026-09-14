use anyhow::{Context, Result, bail};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::consts::{LTX_NESTED_COMMANDS, NO_KEY};
use crate::utils::encoding::{encode_cp1251, read_cp1251_file};

#[derive(Debug, Clone)]
pub struct GameConfig {
  data: HashMap<String, HashMap<String, String>>,
  file_path: String,
  /// Original file lines kept so patch saves do not wipe unrelated cvars/comments.
  /// Each entry is `(line without its ending, the ending that followed it)`.
  /// The ending is stored per line so a file with mixed CRLF/LF is not silently
  /// rewritten to a single style.  The last entry's ending is empty when the
  /// file did not end with a newline.
  raw_lines: Vec<(String, String)>,
  /// Keys changed since last load: (section_or_cvar, nested_or_same).
  dirty: HashSet<(String, String)>,
  /// Keys removed since last load (profile NO_KEY).
  removed: HashSet<(String, String)>,
  /// Line ending detected during load ("\r\n" or "\n").
  line_ending: String,
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
      line_ending: String::from("\n"),
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
    self.line_ending = String::from("\n");

    if !Path::new(&self.file_path).exists() {
      return Ok(());
    }

    // user.ltx is a Windows-1251 file (like alife.ltx): reading it as UTF-8
    // fails outright on any Cyrillic byte (player name, a Russian comment),
    // which used to break BOTH saving run params and preparing the launch.
    let content = read_cp1251_file(&self.file_path).with_context(|| format!("Failed to read config file: {}", self.file_path))?;

    // Dominant line ending — used only for lines the patch save APPENDS.
    // Existing lines keep their own ending (see `raw_lines`).
    self.line_ending = if content.contains("\r\n") { "\r\n" } else { "\n" }.to_string();

    self.raw_lines = split_lines_keep_endings(&content);

    for (line, _) in &self.raw_lines {
      let trimmed = line.trim();
      if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
        continue;
      }

      // Cut the trailing comment BEFORE parsing: otherwise it ended up inside
      // the value and was appended a second time on every save.
      let code = strip_trailing_comment(trimmed);

      // `None` = bare command (e.g. default_controls) — kept via raw_lines, not in data.
      let Some((key, key2, value)) = parse_entry(code) else {
        continue;
      };

      // Merge, never replace: a plain `self.data.insert(key, map)` wiped the
      // whole `bind` map as soon as one bind line parsed as a flat pair.
      self.data.entry(key).or_default().insert(key2, value);
    }

    Ok(())
  }

  /// Patch-save: rewrite only dirty/removed keys; keep comments, bare cmds, untouched cvars.
  pub fn save(&self) -> Result<()> {
    if self.file_path.is_empty() {
      bail!("save() user.ltx read error ! file_path is not set ! Empty string !")
    }

    let source_lines: Vec<(String, String)> = if !self.raw_lines.is_empty() {
      self.raw_lines.clone()
    } else if Path::new(&self.file_path).exists() {
      let content = read_cp1251_file(&self.file_path)
        .with_context(|| format!("Failed to read config file for patch save: {}", self.file_path))?;
      split_lines_keep_endings(&content)
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
    // Also index dirty entries by first token so that flat pairs whose value
    // contains spaces (e.g. `vid_mode 1920x1080`) can be found when the source
    // line is parsed by parse_key_pair as (vid_mode, 1920x1080).
    let mut dirty_by_token: HashMap<String, ((String, String), String)> = HashMap::new();
    for (key1, key2) in &self.dirty {
      if let Some(inner) = self.data.get(key1) {
        if let Some(value) = inner.get(key2) {
          let line = if key1 == key2 {
            format!("{} {}", key1, value)
          } else {
            format!("{} {} {}", key1, key2, value)
          };
          // Only index flat pairs (key1 == key2) by token. Nested pairs like
          // ("bind", "jump") must match exclusively via exact dirty_line lookup;
          // token-based fallback would replace unrelated bindings sharing the
          // same command prefix (R1 regression fix).
          if key1 == key2 {
            dirty_by_token.entry(key1.clone()).or_insert_with(|| ((key1.clone(), key2.clone()), line.clone()));
          }
          dirty_line.insert((key1.clone(), key2.clone()), line);
        }
      }
    }

    // How many source lines start with each command. The token fallback below
    // is only safe for a command that appears exactly once: `bind` appears
    // dozens of times, and rewriting every one of them with the same rebuilt
    // line replaced the player's whole control scheme with a single binding.
    let mut token_counts: HashMap<String, usize> = HashMap::new();
    for (line, _) in &source_lines {
      let trimmed = line.trim();
      if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
        continue;
      }
      if let Some(tok) = first_token(strip_trailing_comment(trimmed)) {
        *token_counts.entry(tok.to_string()).or_insert(0) += 1;
      }
    }

    let mut result: Vec<(String, String)> = Vec::new();
    let mut emitted: HashSet<(String, String)> = HashSet::new();

    for (line, ending) in &source_lines {
      let trimmed = line.trim();
      if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with('#') {
        result.push((line.clone(), ending.clone()));
        continue;
      }

      match parse_key_pair(trimmed) {
        Some(pair) if self.removed.contains(&pair) => {
          // drop removed binding/cvar
        }
        Some(pair) => {
          // Try exact pair match first, then fall back to first-token match
          // for flat pairs whose value contains spaces (e.g. `vid_mode 1920x1080`).
          let dirty_match = dirty_line.get(&pair).map(|v| (&pair, v)).or_else(|| {
            first_token(strip_trailing_comment(trimmed))
              .filter(|tok| token_counts.get(*tok).copied().unwrap_or(0) == 1)
              .and_then(|tok| dirty_by_token.get(tok).map(|(p, v)| (p, v)))
          });
          if let Some((matched_pair, new_value)) = dirty_match {
            let pair = matched_pair.clone();
            // Preserve the original line's indentation prefix
            let prefix_len = line.len() - line.trim_start().len();
            let prefix = &line[..prefix_len];
            // Preserve trailing comment (; ...) if present in the original line,
            // together with the whitespace that separated it from the value.
            let trailing = find_trailing_comment_with_gap(trimmed);
            let mut rebuilt = format!("{}{}", prefix, new_value);
            if let Some(comment) = trailing {
              rebuilt.push_str(comment);
            }
            result.push((rebuilt, ending.clone()));
            emitted.insert(pair);
          } else {
            result.push((line.clone(), ending.clone()));
          }
        }
        None => result.push((line.clone(), ending.clone())),
      }
    }

    for (pair, new_line) in &dirty_line {
      if !emitted.contains(pair) {
        result.push((new_line.clone(), self.line_ending.clone()));
      }
    }

    // Re-assemble with the ORIGINAL per-line endings: joining everything with a
    // single detected ending converted a mixed-ending file wholesale to CRLF.
    // An empty ending means the source line had no newline after it (last line);
    // the file is still written newline-terminated, as before.
    let mut out = String::new();
    for (line, ending) in &result {
      out.push_str(line);
      out.push_str(if ending.is_empty() { &self.line_ending } else { ending });
    }

    // Atomic tmp+rename so a crash mid-save cannot truncate user.ltx.
    let bytes = encode_cp1251(&out).with_context(|| format!("Failed to encode config file as cp1251: {}", self.file_path))?;
    crate::configs::atomic_write_bytes(&self.file_path, &bytes).with_context(|| format!("Failed to write config file: {}", self.file_path))?;

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
    let mut out = lines.join(&self.line_ending);
    if !out.is_empty() {
      out.push_str(&self.line_ending);
    }
    // Atomic tmp+rename so a crash mid-save cannot truncate user.ltx.
    let bytes = encode_cp1251(&out).with_context(|| format!("Failed to encode config file as cp1251: {}", self.file_path))?;
    crate::configs::atomic_write_bytes(&self.file_path, &bytes).with_context(|| format!("Failed to write config file: {}", self.file_path))?;
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

/// Find a trailing comment (`; ...`) in a trimmed line, skipping semicolons
/// inside values. Returns the comment string including the leading `;`.
fn find_trailing_comment(trimmed: &str) -> Option<&str> {
  // Walk past the key-value tokens to find the first unquoted semicolon.
  let mut in_quotes = false;
  let bytes = trimmed.as_bytes();
  for i in 0..bytes.len() {
    match bytes[i] {
      b'"' => in_quotes = !in_quotes,
      b';' if !in_quotes => return Some(&trimmed[i..]),
      _ => {}
    }
  }
  None
}

/// Same as [`find_trailing_comment`], but the returned slice also contains the
/// whitespace that separated the comment from the value, so a rebuilt line
/// keeps its `value ; comment` spacing instead of gluing them together.
fn find_trailing_comment_with_gap(trimmed: &str) -> Option<&str> {
  let comment = find_trailing_comment(trimmed)?;
  let comment_start = trimmed.len() - comment.len();
  let code_len = trimmed[..comment_start].trim_end().len();
  Some(&trimmed[code_len..])
}

/// Drop a trailing comment from a trimmed line, returning only the code part.
fn strip_trailing_comment(trimmed: &str) -> &str {
  match find_trailing_comment(trimmed) {
    Some(comment) => {
      let comment_start = trimmed.len() - comment.len();
      trimmed[..comment_start].trim_end()
    }
    None => trimmed,
  }
}

/// Split a file into `(line, ending)` pairs, keeping each line's own ending
/// ("\r\n", "\n", or "" for a last line without a newline).
fn split_lines_keep_endings(content: &str) -> Vec<(String, String)> {
  let mut out: Vec<(String, String)> = Vec::new();
  let bytes = content.as_bytes();
  let mut start = 0usize;

  for i in 0..bytes.len() {
    if bytes[i] != b'\n' {
      continue;
    }
    let mut end = i;
    let mut ending = "\n";
    if end > start && bytes[end - 1] == b'\r' {
      end -= 1;
      ending = "\r\n";
    }
    out.push((content[start..end].to_string(), ending.to_string()));
    start = i + 1;
  }

  if start < content.len() {
    out.push((content[start..].to_string(), String::new()));
  }

  out
}

/// True for commands whose second token names an action rather than being part
/// of the value (`bind`, `bind_sec`).
fn is_nested_command(key: &str) -> bool {
  LTX_NESTED_COMMANDS.contains(&key)
}

/// Parse ONE comment-free line into `(key1, key2, value)`.
/// `bind jump kSPACE` → `("bind", "jump", "kSPACE")`;
/// `vid_mode 1920x1080` → `("vid_mode", "vid_mode", "1920x1080")`.
/// `None` for a bare command with no value (`default_controls`).
///
/// Nesting is decided by the COMMAND NAME. Counting spaces (the old rule)
/// misparsed `bind jump kSPACE ; comment` and `bind  use kF` as flat pairs.
fn parse_entry(code: &str) -> Option<(String, String, String)> {
  let pos = code.find(char::is_whitespace)?;
  let key = &code[..pos];
  let value = code[pos..].trim_start();
  if value.is_empty() {
    return None;
  }

  if is_nested_command(key) {
    if let Some(pos2) = value.find(char::is_whitespace) {
      let key2 = &value[..pos2];
      let value2 = value[pos2..].trim_start();
      if !value2.is_empty() {
        return Some((key.to_string(), key2.to_string(), value2.to_string()));
      }
    }
    // `bind jump` with no key at all — keep it as a flat pair so the line is
    // still round-tripped instead of being dropped.
  }

  Some((key.to_string(), key.to_string(), value.to_string()))
}

/// Parse a user.ltx line into (key1, key2) using the same rules as load().
fn parse_key_pair(line: &str) -> Option<(String, String)> {
  let (key1, key2, _) = parse_entry(strip_trailing_comment(line))?;
  Some((key1, key2))
}

/// Extract just the first token (command name) from a line.
/// Used in save() to find existing lines by command, regardless of whether
/// the value contains spaces (flat pair) or not (nested pair).
fn first_token(line: &str) -> Option<&str> {
  if let Some(pos) = line.find(char::is_whitespace) {
    Some(&line[..pos])
  } else {
    None
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// R1 regression: applying a keybind profile must not overwrite unrelated
  /// `bind` lines whose action is absent from the dirty set.
  #[test]
  fn save_preserves_unmanaged_bind_lines() {
    let dir = std::env::temp_dir().join("gw_launcher_r1_test");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("user.ltx");

    // Original file: two bind lines and one flat cvar with a space in value.
    let original = "bind jump kSPACE\r\nbind use kF\r\nvid_mode 1920x1080\r\n";
    std::fs::write(&path, encode_cp1251(original).unwrap()).unwrap();

    let mut cfg = GameConfig::new(&path);
    cfg.load().unwrap();

    // Simulate profile application: only "jump" binding is dirty.
    // set() creates data[key][key] = value; for nested pairs we manipulate
    // data directly to set data["bind"]["jump"] = "kRETURN".
    {
      let bind_map = cfg.data.entry("bind".to_string()).or_default();
      bind_map.insert("jump".to_string(), "kRETURN".to_string());
    }
    cfg.dirty.insert(("bind".to_string(), "jump".to_string()));
    // Also dirty vid_mode (flat pair with space in value).
    cfg.set("vid_mode".to_string(), "vid_mode".to_string());
    cfg.data.get_mut("vid_mode").unwrap().insert("vid_mode".to_string(), "1280x720".to_string());
    cfg.dirty.insert(("vid_mode".to_string(), "vid_mode".to_string()));

    cfg.save().unwrap();

    let saved = read_cp1251_file(&path).unwrap();
    // The unmanaged "bind use kF" line must survive byte-for-byte.
    assert!(saved.contains("bind use kF"), "unmanaged bind line lost: {}", saved);
    // The managed "bind jump" must be updated.
    assert!(saved.contains("bind jump kRETURN"), "bind jump not updated: {}", saved);
    // No duplicate "bind use" lines.
    let use_count = saved.lines().filter(|l| l.starts_with("bind use")).count();
    assert_eq!(use_count, 1, "duplicate bind use lines: {}", saved);
    // vid_mode must be updated, not duplicated.
    let vid_count = saved.lines().filter(|l| l.starts_with("vid_mode")).count();
    assert_eq!(vid_count, 1, "vid_mode duplicated: {}", saved);

    let _ = std::fs::remove_file(&path);
  }

  fn write_ltx(name: &str, content: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join("gw_launcher_r14_test");
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join(name);
    std::fs::write(&path, encode_cp1251(content).unwrap()).unwrap();
    path
  }

  /// R14 + 85 + 86 + 87 regression, driven through the REAL parser (the older
  /// test filled `data` by hand and therefore never exercised `load()`).
  ///
  /// A bind line with a trailing comment, a bind line with a double space and a
  /// flat cvar with a double space used to parse as flat pairs, wiping the whole
  /// `bind` map and reviving the token fallback in `save()`.
  #[test]
  fn load_parses_comments_and_double_spaces() {
    let path = write_ltx(
      "parse.ltx",
      "bind jump kSPACE ; коммент\r\nbind  use kF\r\nvid_mode  1920x1080\r\nbind_sec jump kNUMPAD0\r\ndefault_controls\r\n",
    );

    let mut cfg = GameConfig::new(&path);
    cfg.load().unwrap();

    let binds = cfg.get("bind").expect("bind map must exist");
    assert_eq!(binds.get("jump").map(String::as_str), Some("kSPACE"), "comment leaked into the value or the bind map was wiped: {:?}", binds);
    assert_eq!(binds.get("use").map(String::as_str), Some("kF"), "double-space bind line lost: {:?}", binds);
    assert_eq!(binds.len(), 2, "unexpected bind entries: {:?}", binds);

    let sec = cfg.get("bind_sec").expect("bind_sec map must exist");
    assert_eq!(sec.get("jump").map(String::as_str), Some("kNUMPAD0"));

    // 85: `vid_mode  1920x1080` used to produce an EMPTY inner map.
    let vid = cfg.get("vid_mode").expect("vid_mode must exist");
    assert_eq!(vid.get("vid_mode").map(String::as_str), Some("1920x1080"), "vid_mode value lost: {:?}", vid);

    // A bare command is not data, but must survive the round trip.
    assert!(cfg.get("default_controls").is_none());

    let _ = std::fs::remove_file(&path);
  }

  /// load() + save() without any change must rewrite the file byte-for-byte,
  /// mixed line endings included (87).
  #[test]
  fn load_save_roundtrip_is_byte_identical() {
    let original = "bind jump kSPACE ; коммент\r\nbind  use kF\nvid_mode  1920x1080\r\n; голый комментарий\ndefault_controls\r\n";
    let path = write_ltx("roundtrip.ltx", original);

    let mut cfg = GameConfig::new(&path);
    cfg.load().unwrap();
    cfg.save().unwrap();

    let saved = std::fs::read(&path).unwrap();
    assert_eq!(saved, encode_cp1251(original).unwrap(), "round trip changed the file: {:?}", read_cp1251_file(&path).unwrap());

    let _ = std::fs::remove_file(&path);
  }

  /// Applying one binding after a real `load()` must touch exactly that line:
  /// no duplicates, no other bind rewritten, the comment appended once.
  #[test]
  fn apply_after_load_touches_only_the_changed_binding() {
    let original = "bind jump kSPACE ; коммент\r\nbind  use kF\r\nbind crouch kLCONTROL\r\nvid_mode  1920x1080\r\n";
    let path = write_ltx("apply.ltx", original);

    let mut cfg = GameConfig::new(&path);
    cfg.load().unwrap();
    cfg.set2("bind".to_string(), "jump".to_string(), "kRETURN".to_string());
    cfg.save().unwrap();

    let saved = read_cp1251_file(&path).unwrap();

    assert_eq!(saved.lines().filter(|l| l.starts_with("bind jump")).count(), 1, "bind jump duplicated: {}", saved);
    assert!(saved.contains("bind jump kRETURN"), "bind jump not updated: {}", saved);
    // 86: the comment must appear exactly once, and only on its own line.
    assert_eq!(saved.matches("коммент").count(), 1, "comment duplicated: {}", saved);
    // R14: the other bindings must be untouched by the token fallback.
    assert!(saved.contains("bind  use kF"), "bind use rewritten or lost: {}", saved);
    assert!(saved.contains("bind crouch kLCONTROL"), "bind crouch rewritten or lost: {}", saved);
    assert_eq!(saved.lines().filter(|l| l.trim_start().starts_with("bind ")).count(), 3, "bind line count changed: {}", saved);
    // 85: the flat cvar keeps its value.
    assert!(saved.contains("vid_mode  1920x1080"), "vid_mode lost: {}", saved);

    let _ = std::fs::remove_file(&path);
  }
}
