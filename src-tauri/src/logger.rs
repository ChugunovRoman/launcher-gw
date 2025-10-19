use log::{Level, Metadata, Record};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum LogLevel {
  #[serde(rename = "debug")]
  Debug,
  #[serde(rename = "info")]
  Info,
  #[serde(rename = "warn")]
  Warn,
  #[serde(rename = "error")]
  Error,
}

impl LogLevel {
  pub fn from_str(s: &str) -> Option<Self> {
    match s.to_lowercase().as_str() {
      "debug" => Some(LogLevel::Debug),
      "info" => Some(LogLevel::Info),
      "warn" => Some(LogLevel::Warn),
      "error" => Some(LogLevel::Error),
      _ => None,
    }
  }

  pub fn as_str(&self) -> &'static str {
    match self {
      LogLevel::Debug => "DEBUG",
      LogLevel::Info => "INFO",
      LogLevel::Warn => "WARN",
      LogLevel::Error => "ERROR",
    }
  }
}

impl Default for LogLevel {
  fn default() -> Self {
    LogLevel::Info
  }
}

#[derive(Clone)]
pub struct Logger {
  log_file_path: PathBuf,
  min_level: LogLevel,
}

impl Logger {
  pub fn new(min_level: LogLevel) -> Result<Self, Box<dyn std::error::Error>> {
    let log_dir = std::env::current_dir()
      .unwrap_or_else(|_| PathBuf::from("."))
      .join("appdata")
      .join("logs");

    std::fs::create_dir_all(&log_dir)?;

    let log_file_path = log_dir.join("launcher.log");

    std::fs::remove_file(&log_file_path).unwrap_or_else(|_| println!("Cannot remove log file"));

    // Убедимся, что файл существует
    std::fs::OpenOptions::new()
      .create(true)
      .write(true)
      .append(true)
      .open(&log_file_path)?;

    Ok(Logger {
      log_file_path,
      min_level,
    })
  }

  fn should_log(&self, level: &LogLevel) -> bool {
    match (&self.min_level, level) {
      (LogLevel::Debug, _) => true,
      (LogLevel::Info, LogLevel::Info | LogLevel::Warn | LogLevel::Error) => true,
      (LogLevel::Warn, LogLevel::Warn | LogLevel::Error) => true,
      (LogLevel::Error, LogLevel::Error) => true,
      _ => false,
    }
  }

  fn write_log(&self, level: LogLevel, message: &str) {
    if !self.should_log(&level) {
      return;
    }

    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let line = format!("[{}] {} - {}\n", timestamp, level.as_str(), message);

    // Открываем, пишем, закрываем — как вы просили
    let mut file = OpenOptions::new()
      .write(true)
      .append(true)
      .open(&self.log_file_path)
      .expect("Failed to open log file");

    print!("{}", &line);

    if let Err(e) = writeln!(file, "{}", line.trim_end()) {
      eprintln!("Failed to write to log: {}", e);
    }
    // Файл автоматически закрывается при выходе из scope
  }

  pub fn debug(&self, message: &str) {
    self.write_log(LogLevel::Debug, message);
  }

  pub fn info(&self, message: &str) {
    self.write_log(LogLevel::Info, message);
  }

  pub fn warn(&self, message: &str) {
    self.write_log(LogLevel::Warn, message);
  }

  pub fn error(&self, message: &str) {
    self.write_log(LogLevel::Error, message);
  }

  /// Обновить уровень логирования
  pub fn set_level(&mut self, level: LogLevel) {
    self.min_level = level;
  }

  /// Получить текущий путь к лог-файлу (для отладки или экспорта)
  pub fn log_path(&self) -> &Path {
    &self.log_file_path
  }
}

pub struct TauriLogger {
  pub inner: Arc<Mutex<Logger>>,
}

impl log::Log for TauriLogger {
  fn enabled(&self, metadata: &Metadata) -> bool {
    true
  }

  fn log(&self, record: &Record) {
    if !self.enabled(record.metadata()) {
      return;
    }
    if let Ok(logger) = self.inner.lock() {
      let msg = format!("{} - {}", record.target(), record.args());
      match record.level() {
        Level::Error => logger.error(&msg),
        Level::Warn => logger.warn(&msg),
        Level::Info => logger.info(&msg),
        _ => logger.debug(&msg),
      }
    }
  }

  fn flush(&self) {}
}
