use std::{
  panic,
  sync::{Arc, Mutex},
};

use crate::logger::Logger;

pub fn setup_panic_logger(logger: Arc<Mutex<Logger>>) {
  panic::set_hook(Box::new(move |info| {
    let msg = match info.payload().downcast_ref::<&str>() {
      Some(s) => s.to_string(),
      None => match info.payload().downcast_ref::<String>() {
        Some(s) => s.clone(),
        None => "Box<dyn Any>".to_string(),
      },
    };

    let location = info
      .location()
      .map(|loc| format!(" at {}:{}:{}", loc.file(), loc.line(), loc.column()))
      .unwrap_or_default();

    let full_msg = format!("PANIC: {}{}", msg, location);

    if let Ok(logger) = logger.lock() {
      logger.error(&full_msg);
    }

    eprintln!("{}", full_msg);
  }));
}
