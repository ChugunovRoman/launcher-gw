use std::env;
use std::fs;
use std::path::Path;

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
