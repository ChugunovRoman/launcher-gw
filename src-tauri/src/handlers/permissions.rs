use std::sync::Arc;
use tokio::sync::Mutex;

use crate::service::dto::UserData;

#[tauri::command]
pub async fn allow_pack_mod(user_data: tauri::State<'_, Arc<Mutex<Option<UserData>>>>) -> Result<bool, String> {
  let data = user_data.lock().await;

  Ok(data.as_ref().map_or(false, |user| user.flags.iter().any(|value| value == "allowPackMod")))
}
