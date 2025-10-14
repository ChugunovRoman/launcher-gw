mod handlers;

use handlers::register_window_handlers;
use tauri::{Builder, Wry};

fn create_tauri_app() -> Builder<Wry> {
  let mut app = tauri::Builder::default();
  app = register_window_handlers(app);
  return app;
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  create_tauri_app()
    .plugin(tauri_plugin_opener::init())
    .plugin(tauri_plugin_window_state::Builder::default().build())
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
  // let args: Vec<String> = std::env::args().collect();

  // println!("Hello, world!, args: {:?}", args);
}
