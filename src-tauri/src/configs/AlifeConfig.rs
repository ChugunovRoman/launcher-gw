#![allow(non_snake_case)]

use anyhow::Result;
use std::path::Path;

use crate::configs::LtxLines::{LtxEncoding, LtxLines};

/// cp1251 sectioned-ltx patcher for the engine's own configs (`alife.ltx`,
/// `axr_options.ltx`).
///
/// Thin wrapper over `LtxLines`, which holds the line-oriented implementation
/// shared with the UTF-8 faction-editor configs. Comments, line order,
/// indentation, key alignment and line endings are preserved as-is.
#[derive(Debug, Clone)]
pub struct AlifeConfig(LtxLines);

impl AlifeConfig {
  /// Load an existing file. `Ok(None)` — file does not exist (must not be created).
  pub fn load<P: AsRef<Path>>(path: P) -> Result<Option<Self>> {
    Ok(LtxLines::load(path, LtxEncoding::Cp1251)?.map(Self))
  }

  /// Write `key = value` into `section`.
  /// `false` — the section is missing from the file, nothing was changed.
  pub fn set_in_section(&mut self, section: &str, key: &str, value: &str) -> bool {
    self.0.set_in_section(section, key, value)
  }

  /// `true` if a `[section]` header exists (even with no keys under it).
  pub fn has_section(&self, section: &str) -> bool {
    self.0.has_section(section)
  }

  /// Read all `key = value` pairs of a section (comments stripped, keys as
  /// written in the file). Empty — the section is missing or empty. Used to
  /// build a fragment (e.g. `axr_options.partial.ltx`) without touching the
  /// rest of the file.
  pub fn get_section(&self, section: &str) -> Vec<(String, String)> {
    self.0.get_section(section)
  }

  /// Atomically save the file in cp1251 with the original line endings.
  pub fn save(&self) -> Result<()> {
    self.0.save()
  }
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
