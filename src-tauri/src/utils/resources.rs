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

// Known Stalker launcher stub stems (Stalker-CoC.exe etc.). The launcher never
// executes them: they are compiled AutoHotkey wrappers that check the CoP
// registry keys and ShellExecute the engine with UAC elevation (RunAs). Used
// only to filter such stubs out of `version.exe_path` (manifest tier 1).
pub const STALKER_LAUNCHER_STEMS: &[&str] = &["Stalker-CoC", "Stalker-CoP", "Stalker-CS", "Stalker"];
