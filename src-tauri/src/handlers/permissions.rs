use std::sync::{Arc, Mutex};

use crate::gitlab::models::UserData;

#[tauri::command]
pub fn allow_pack_mod(user_data: tauri::State<'_, Arc<Mutex<UserData>>>) -> Result<bool, String> {
  let data = user_data.lock().map_err(|_| "Failed to lock user config")?;

  let has = data.flags.iter().any(|value| value == "allowPackMod");

  Ok(has)
}
