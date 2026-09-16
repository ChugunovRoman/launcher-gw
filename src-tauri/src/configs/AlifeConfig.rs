#![allow(non_snake_case)]

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::utils::encoding::{encode_cp1251, read_cp1251_file};

/// Indent and key padding as written by CInifile::save_as (`%8s%-32s = %s`).
const KEY_INDENT: &str = "        ";
const KEY_WIDTH: usize = 32;

/// Line-oriented patcher for sectioned ltx files (`[section]` + `key = value`).
/// Patches only the requested keys; comments, line order, indentation, key
/// alignment, encoding and line endings are preserved as-is.
#[derive(Debug, Clone)]
pub struct AlifeConfig {
  path: PathBuf,
  lines: Vec<String>,
  eol: &'static str,
}

impl AlifeConfig {
  /// Load an existing file. `Ok(None)` — file does not exist (must not be created).
  pub fn load<P: AsRef<Path>>(path: P) -> Result<Option<Self>> {
    let path = path.as_ref().to_path_buf();
    if !path.exists() {
      return Ok(None);
    }

    let content = read_cp1251_file(&path).with_context(|| format!("Не удалось прочитать {}", path.display()))?;
    let eol = if content.contains("\r\n") { "\r\n" } else { "\n" };
    let lines: Vec<String> = content.split('\n').map(|l| l.trim_end_matches('\r').to_string()).collect();

    Ok(Some(Self { path, lines, eol }))
  }

  /// Write `key = value` into `section`.
  /// `false` — the section is missing from the file, nothing was changed.
  pub fn set_in_section(&mut self, section: &str, key: &str, value: &str) -> bool {
    let Some((start, end)) = self.section_range(section) else {
      return false;
    };

    // 1) The key already exists in the section — replace the value only.
    for i in start..end {
      let line = self.lines[i].clone();
      let payload = match line.find(';') {
        Some(pos) => &line[..pos],
        None => &line[..],
      };
      let comment = match line.find(';') {
        Some(pos) => Some(line[pos..].to_string()),
        None => None,
      };

      let Some(eq) = payload.find('=') else { continue };
      if !payload[..eq].trim().eq_ignore_ascii_case(key) {
        continue;
      }

      // `head` keeps the original prefix: all padding spaces and the `=` sign.
      let head = &payload[..=eq];
      let mut new_line = format!("{} {}", head, value);
      if let Some(comment) = comment {
        new_line.push_str("  ");
        new_line.push_str(&comment);
      }
      self.lines[i] = new_line;
      return true;
    }

    // 2) The key is missing — insert after the last non-empty line of the section.
    let mut insert_at = end;
    while insert_at > start && self.lines[insert_at - 1].trim().is_empty() {
      insert_at -= 1;
    }
    self.lines.insert(insert_at, format!("{}{:<width$} = {}", KEY_INDENT, key, value, width = KEY_WIDTH));
    true
  }

  /// `true` if a `[section]` header exists (even with no keys under it).
  pub fn has_section(&self, section: &str) -> bool {
    self.section_range(section).is_some()
  }

  /// Read all `key = value` pairs of a section (comments stripped, keys as
  /// written in the file). Empty — the section is missing or empty. Used to
  /// build a fragment (e.g. `axr_options.partial.ltx`) without touching the
  /// rest of the file.
  pub fn get_section(&self, section: &str) -> Vec<(String, String)> {
    let Some((start, end)) = self.section_range(section) else {
      return Vec::new();
    };

    let mut out = Vec::new();
    for line in &self.lines[start..end] {
      let payload = match line.find(';') {
        Some(pos) => &line[..pos],
        None => &line[..],
      };
      let Some(eq) = payload.find('=') else { continue };
      let key = payload[..eq].trim();
      let value = payload[eq + 1..].trim();
      if key.is_empty() {
        continue;
      }
      out.push((key.to_string(), value.to_string()));
    }
    out
  }

  /// Atomically save the file in cp1251 with the original line endings.
  pub fn save(&self) -> Result<()> {
    let mut out = self.lines.join(self.eol);
    if !out.ends_with(self.eol) {
      out.push_str(self.eol);
    }
    let bytes = encode_cp1251(&out)?;
    crate::configs::atomic_write_bytes(&self.path, &bytes)
  }

  /// Line range of the section body: `[start, end)` — from the line after the
  /// header up to the next section header (or end of file).
  fn section_range(&self, section: &str) -> Option<(usize, usize)> {
    let mut start: Option<usize> = None;

    for (i, line) in self.lines.iter().enumerate() {
      let Some(name) = parse_section_header(line) else { continue };

      if start.is_some() {
        return Some((start.unwrap(), i));
      }
      if name.eq_ignore_ascii_case(section) {
        start = Some(i + 1);
      }
    }

    start.map(|s| (s, self.lines.len()))
  }
}

/// `[alife]` -> `alife`; `[alife]:base1,base2` -> `alife`; otherwise `None`.
fn parse_section_header(line: &str) -> Option<&str> {
  let trimmed = line.trim();
  if !trimmed.starts_with('[') {
    return None;
  }
  let close = trimmed.find(']')?;
  Some(trimmed[1..close].trim())
}

#[cfg(test)]
mod tests {
  use super::*;

  // Mirrors the engine-written shape of gamedata/configs/alife.ltx: CRLF,
  // 8-space indent, key padded to 32 columns, trailing " " line.
  const SAMPLE: &str = "[alife]\r\n        objects_per_update               = 20\r\n        start_time                       = 10:00:00\r\n \r\n";

  /// A pid-only name collided across tests once a second test using this
  /// helper (`get_section_reads_keys_without_mutating`) started running
  /// concurrently with `replaces_value_and_keeps_formatting` — both threads
  /// wrote/removed the very same file. Add a per-call counter so each caller
  /// gets its own path.
  fn temp_ltx() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("alife_config_test_{}_{}.ltx", std::process::id(), n));
    let bytes = crate::utils::encoding::encode_cp1251(SAMPLE).unwrap();
    std::fs::write(&path, bytes).unwrap();
    path
  }

  #[test]
  fn replaces_value_and_keeps_formatting() {
    let path = temp_ltx();
    let mut ltx = AlifeConfig::load(&path).unwrap().unwrap();

    assert!(ltx.set_in_section("alife", "objects_per_update", "40"));
    assert!(ltx.set_in_section("ALIFE", "switch_distance", "150"));
    assert!(!ltx.set_in_section("missing_section", "key", "value"));
    ltx.save().unwrap();

    let bytes = std::fs::read(&path).unwrap();
    assert!(!std::path::Path::new(&path.with_extension("ltx.tmp")).exists(), "temp file must be gone");
    let text = String::from_utf8_lossy(&bytes).to_string();
    assert!(text.contains("\r\n"), "CRLF must survive");
    assert!(text.contains("[alife]\r\n"), "single header must survive");
    // Engine alignment: 8 spaces + key padded to 32 + " = ".
    assert!(text.contains("        objects_per_update               = 40\r\n"));
    assert!(text.contains("        switch_distance                  = 150\r\n"));
    // Untouched keys and the trailing " " line must stay byte-identical.
    assert!(text.contains("        start_time                       = 10:00:00\r\n"));
    assert!(text.ends_with(" \r\n"));
    std::fs::remove_file(&path).ok();
  }

  #[test]
  fn get_section_reads_keys_without_mutating() {
    let path = temp_ltx();
    let ltx = AlifeConfig::load(&path).unwrap().unwrap();

    let pairs = ltx.get_section("alife");
    assert_eq!(
      pairs,
      vec![
        ("objects_per_update".to_string(), "20".to_string()),
        ("start_time".to_string(), "10:00:00".to_string()),
      ]
    );
    assert!(ltx.get_section("missing_section").is_empty());
    std::fs::remove_file(&path).ok();
  }

  #[test]
  fn missing_file_is_not_created() {
    let path = std::env::temp_dir().join("alife_config_test_missing.ltx");
    std::fs::remove_file(&path).ok();
    assert!(AlifeConfig::load(&path).unwrap().is_none());
    assert!(!path.exists());
  }
}
