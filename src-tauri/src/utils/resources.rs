use std::{env, fs, path::PathBuf};

#[cfg(windows)]
const SEVEN_ZIP_BIN: &[u8] = include_bytes!("../../../bin/win/7za.exe");
#[cfg(windows)]
const SEVEN_ZZA_DLL: &[u8] = include_bytes!("../../../bin/win/7za.dll");
#[cfg(windows)]
const SEVEN_ZZXA_DLL: &[u8] = include_bytes!("../../../bin/win/7zxa.dll");
#[cfg(unix)]
const SEVEN_ZIP_BIN: &[u8] = include_bytes!("../../../bin/linux/7zzs");

pub fn get_7zip_path() -> PathBuf {
  // Используем временную директорию ОС
  let mut temp_path = env::temp_dir();
  let mut temp_path_dll_1 = env::temp_dir();
  let mut temp_path_dll_2 = env::temp_dir();

  #[cfg(windows)]
  temp_path.push("7za.exe");
  #[cfg(windows)]
  temp_path_dll_1.push("7za.dll");
  #[cfg(windows)]
  temp_path_dll_2.push("7zxa.dll");
  #[cfg(unix)]
  temp_path.push("7zzs");

  // Если файла еще нет в темпе, записываем его туда
  if !temp_path.exists() {
    fs::write(&temp_path, SEVEN_ZIP_BIN).expect("Failed to write 7zip binary to temp");

    #[cfg(windows)]
    fs::write(&temp_path_dll_1, SEVEN_ZZA_DLL).expect("Failed to write 7zip binary to temp");
    #[cfg(windows)]
    fs::write(&temp_path_dll_2, SEVEN_ZZXA_DLL).expect("Failed to write 7zip binary to temp");

    // На Linux/macOS нужно дать права на выполнение
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      let mut perms = fs::metadata(&temp_path).unwrap().permissions();
      perms.set_mode(0o755);
      fs::set_permissions(&temp_path, perms).unwrap();
    }
  }

  temp_path
}

pub fn game_exe() -> String {
  let binary_name = if cfg!(windows) { "xrEngine.exe".to_owned() } else { "xr_3da".to_owned() };

  binary_name
}

pub fn launcher_exe() -> String {
  let binary_name = if cfg!(windows) {
    "Launcher.exe".to_owned()
  } else {
    "Launcher".to_owned()
  };

  binary_name
}
