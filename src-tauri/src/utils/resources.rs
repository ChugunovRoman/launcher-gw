use crate::consts::*;
use std::path::{Path, PathBuf};

pub fn game_exe() -> String {
  let binary_name = if cfg!(windows) { "xrEngine.exe".to_owned() } else { "xr_3da".to_owned() };

  binary_name
}

pub fn launcher_exe() -> String {
  let binary_name = if cfg!(windows) {
    EXE_WIN_NAME.to_owned()
  } else {
    EXE_LINUX_NAME.to_owned()
  };

  binary_name
}

// Candidate Stalker launcher exe stems, checked in priority order.
pub const STALKER_LAUNCHER_STEMS: &[&str] = &["Stalker-CoC", "Stalker-CoP", "Stalker-CS", "Stalker"];

// Appends ".exe" on Windows, leaves the stem as-is on other OS.
// "Stalker-CoC" -> "Stalker-CoC.exe" (Windows) / "Stalker-CoC" (Linux/macOS)
pub fn with_exe_ext(stem: &str) -> String {
  if cfg!(windows) {
    format!("{stem}.exe")
  } else {
    stem.to_string()
  }
}

// Returns the first existing Stalker launcher exe inside `dir` (CoC, CoP, CS, Stalker).
pub fn find_stalker_launcher(dir: &Path) -> Option<PathBuf> {
  for stem in STALKER_LAUNCHER_STEMS {
    let candidate = dir.join(with_exe_ext(stem));
    if candidate.exists() {
      return Some(candidate);
    }
  }
  None
}
