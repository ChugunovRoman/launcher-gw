use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

pub fn clear_dir<P: AsRef<Path>>(dir: P) -> std::io::Result<()> {
  for entry in fs::read_dir(dir)? {
    let entry = entry?;
    let path = entry.path();
    if path.is_dir() {
      fs::remove_dir_all(&path)?;
    } else {
      fs::remove_file(&path)?;
    }
  }
  Ok(())
}

pub fn get_exe_name() -> Option<String> {
  env::current_exe()
    .ok()
    .and_then(|path| path.file_name().and_then(|n| n.to_str()).map(|s| s.to_string()))
}

pub fn get_file_name<P: AsRef<Path>>(output_path: P) -> Option<String> {
  output_path.as_ref().file_name().and_then(|os_str| os_str.to_str().map(|s| s.to_string()))
}

/// Join `base` with a remote asset name, rejecting path traversal (`..`, absolute, nested).
pub fn safe_download_join(base: &Path, remote_name: &str) -> Result<PathBuf, String> {
  let name = Path::new(remote_name)
    .file_name()
    .ok_or_else(|| format!("invalid download file name: {}", remote_name))?;
  if name != Path::new(remote_name).as_os_str() {
    return Err(format!("path traversal rejected in download name: {}", remote_name));
  }
  let joined = base.join(name);
  if !joined.starts_with(base) {
    return Err(format!("download path escapes base dir: {}", remote_name));
  }
  Ok(joined)
}

/// Allow only known release CDN hosts (SSRF guard for blob downloads).
pub fn assert_download_url_allowed(url: &str) -> Result<()> {
  let parsed = url::Url::parse(url).map_err(|e| anyhow::anyhow!("invalid download URL: {}", e))?;
  if parsed.scheme() != "https" && parsed.scheme() != "http" {
    bail!("download URL scheme not allowed: {}", parsed.scheme());
  }
  let host = parsed
    .host_str()
    .ok_or_else(|| anyhow::anyhow!("download URL has no host"))?
    .to_ascii_lowercase();

  let allowed_exact = [
    "github.com",
    "api.github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "gitlab.com",
  ];
  if allowed_exact.iter().any(|h| host == *h)
    || host.ends_with(".githubusercontent.com")
    || host.ends_with(".gitlab.com")
    || host.ends_with(".github.com")
  {
    return Ok(());
  }

  bail!("download URL host not allowed: {}", host)
}

/// user.ltx path must be `.../appdata/user.ltx` (blocks arbitrary IPC writes).
pub fn assert_user_ltx_path(path: &Path) -> Result<(), String> {
  let file = path
    .file_name()
    .and_then(|n| n.to_str())
    .ok_or_else(|| "invalid user.ltx path".to_string())?;
  if !file.eq_ignore_ascii_case("user.ltx") {
    return Err(format!("expected user.ltx, got: {}", file));
  }
  let parent_name = path
    .parent()
    .and_then(|p| p.file_name())
    .and_then(|n| n.to_str())
    .unwrap_or("");
  if !parent_name.eq_ignore_ascii_case("appdata") {
    return Err("user.ltx must live under an appdata directory".to_string());
  }
  if !path.exists() {
    return Err(format!("file not found: {}", path.display()));
  }
  Ok(())
}
