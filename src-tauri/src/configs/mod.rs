pub mod AppConfig;
pub mod GameConfig;

pub use AppConfig::RunParams;
pub use GameConfig::{TmpLtx, UserLtx};

use anyhow::{Context, Result};

/// Write a config file atomically: dump the payload into a temp file next to
/// the target, then rename over the target (mirrors the http_cache pattern).
/// A crash mid-write can then no longer leave a truncated config.json/user.ltx.
pub fn atomic_write(path: &str, data: &str) -> Result<()> {
  let tmp_path = format!("{}.tmp", path);
  std::fs::write(&tmp_path, data).with_context(|| format!("Failed to write temp file: {}", tmp_path))?;
  std::fs::rename(&tmp_path, path).with_context(|| format!("Failed to replace config file: {}", path))?;
  Ok(())
}
