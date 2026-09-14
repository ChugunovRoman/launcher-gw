pub mod AlifeConfig;
pub mod AppConfig;
pub mod GameConfig;

pub use AppConfig::RunParams;
pub use GameConfig::{TmpLtx, UserLtx};

use anyhow::{Context, Result};

/// Write a config file atomically: dump the payload into a temp file next to
/// the target, then rename over the target (mirrors the http_cache pattern).
/// A crash mid-write can then no longer leave a truncated config.json/user.ltx.
pub fn atomic_write(path: &str, data: &str) -> Result<()> {
  // Include the pid: a second launcher instance writing the same config would
  // otherwise share this exact temp file, and the interleaved writes could be
  // renamed over the real config as a corrupt mix.
  let tmp_path = format!("{}.{}.tmp", path, std::process::id());
  std::fs::write(&tmp_path, data).with_context(|| format!("Failed to write temp file: {}", tmp_path))?;
  std::fs::rename(&tmp_path, path).with_context(|| format!("Failed to replace config file: {}", path))?;
  Ok(())
}

/// Byte-oriented variant of `atomic_write` (needed for cp1251 files that must
/// not round-trip through lossy UTF-8 text).
pub fn atomic_write_bytes<P: AsRef<std::path::Path>>(path: P, data: &[u8]) -> Result<()> {
  let path = path.as_ref();
  let tmp_path = path.with_extension(format!("ltx.{}.tmp", std::process::id()));
  std::fs::write(&tmp_path, data).with_context(|| format!("Failed to write temp file: {}", tmp_path.display()))?;
  std::fs::rename(&tmp_path, path).with_context(|| format!("Failed to replace config file: {}", path.display()))?;
  Ok(())
}
