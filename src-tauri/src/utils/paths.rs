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

// ---------------------------------------------------------------------------
// IPC destination guards
//
// `move_version` copies with OverwriteAll and `open_explorer` can create
// directories from raw IPC strings — both need validation against
// destructive targets before touching the filesystem.
// ---------------------------------------------------------------------------

/// Canonicalize a target that may not exist yet: canonicalize the existing
/// parent and re-attach the final component.
fn canonicalize_target(path: &Path) -> Result<PathBuf, String> {
  if let Ok(p) = fs::canonicalize(path) {
    return Ok(p);
  }
  let file_name = path
    .file_name()
    .ok_or_else(|| format!("path has no final component: {}", path.display()))?;
  let parent = path
    .parent()
    .filter(|p| !p.as_os_str().is_empty())
    .ok_or_else(|| format!("path has no parent directory: {}", path.display()))?;
  let canon_parent = fs::canonicalize(parent)
    .map_err(|e| format!("parent directory not accessible: {} ({})", parent.display(), e))?;
  Ok(canon_parent.join(file_name))
}

/// True for locations that must never be targets of an IPC-driven directory
/// creation or an OverwriteAll move. Drive roots and the system roots are
/// blocked exactly; the whole Windows directory subtree is blocked.
fn is_sensitive_directory(canon: &Path) -> bool {
  // Drive root (`\\?\D:\` after canonicalization has no parent).
  if canon.parent().is_none() {
    return true;
  }

  #[cfg(target_os = "windows")]
  {
    fn canon_env(var: &str) -> Option<PathBuf> {
      env::var_os(var).and_then(|v| fs::canonicalize(Path::new(&v)).ok())
    }

    let mut exact: Vec<PathBuf> = Vec::new();
    if let Some(p) = canon_env("ProgramFiles") {
      exact.push(p);
    }
    if let Some(p) = canon_env("ProgramFiles(x86)") {
      exact.push(p);
    }
    if let Some(p) = canon_env("ProgramData") {
      exact.push(p);
    }
    if let Some(profile) = canon_env("USERPROFILE") {
      exact.push(profile.clone());
      // The shared profiles root (e.g. C:\Users) — its SUBTREE stays allowed
      // so games installed under C:\Users\name\Games keep working.
      if let Some(users) = profile.parent() {
        exact.push(users.to_path_buf());
      }
    }
    if exact.iter().any(|p| p == canon) {
      return true;
    }
    if let Some(windows) = canon_env("SystemRoot") {
      if canon.starts_with(&windows) {
        return true;
      }
    }
  }

  #[cfg(not(target_os = "windows"))]
  {
    let exact = ["/usr", "/etc", "/bin", "/sbin", "/var", "/home", "/root", "/boot", "/dev", "/proc", "/sys"];
    if exact.iter().any(|p| Path::new(*p) == canon) {
      return true;
    }
  }

  false
}

/// Validate an IPC-provided destination for `move_version` (which copies
/// with OverwriteAll): absolute path, accessible, not a system location,
/// and not inside the moving version itself.
pub fn assert_move_destination(src: &Path, dest: &Path) -> Result<(), String> {
  if dest.as_os_str().is_empty() {
    return Err("destination path is empty".to_string());
  }
  if !dest.is_absolute() {
    return Err(format!("destination must be an absolute path: {}", dest.display()));
  }
  if dest.exists() && !dest.is_dir() {
    return Err(format!("destination exists and is not a directory: {}", dest.display()));
  }

  let src_canon = fs::canonicalize(src)
    .map_err(|e| format!("source version directory not accessible: {} ({})", src.display(), e))?;
  let dest_canon = canonicalize_target(dest)?;

  if dest_canon == src_canon {
    return Err(format!("destination is the same as the version folder: {}", dest.display()));
  }
  if dest_canon.starts_with(&src_canon) {
    return Err(format!("destination must not be inside the version folder: {}", dest.display()));
  }
  if is_sensitive_directory(&dest_canon) {
    return Err(format!("refusing to move into a system directory: {}", dest.display()));
  }
  Ok(())
}

/// Guard for IPC-driven directory creation (`open_explorer` with createDir):
/// only absolute, non-system targets are allowed. For not-yet-existing paths
/// the parent directory must already exist (no deep tree creation via IPC).
pub fn assert_creatable_directory(path: &Path) -> Result<(), String> {
  if path.as_os_str().is_empty() {
    return Err("path is empty".to_string());
  }
  if !path.is_absolute() {
    return Err(format!("path must be absolute: {}", path.display()));
  }
  let canon = canonicalize_target(path)?;
  if is_sensitive_directory(&canon) {
    return Err(format!("refusing to create or open a system directory: {}", path.display()));
  }
  Ok(())
}
