use crate::consts::*;

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
