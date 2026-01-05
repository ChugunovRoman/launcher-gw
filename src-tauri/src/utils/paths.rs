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

pub fn get_file_name<P: AsRef<Path>>(output_path: P) -> Option<String> {
  output_path.as_ref().file_name().and_then(|os_str| os_str.to_str().map(|s| s.to_string()))
}
