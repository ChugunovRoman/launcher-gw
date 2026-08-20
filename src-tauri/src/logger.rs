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
  /// None — no writable log file found; console-only logging.
  log_file_path: Option<PathBuf>,
  min_level: LogLevel,
}

impl Logger {
  /// Never fails: prefers CWD, falls back to the temp dir (CWD may be
  /// read-only, e.g. Program Files or a service spawn), then degrades to
  /// console-only. The logger must not take the whole app down.
  pub fn new(min_level: LogLevel) -> Self {
    let candidates = [
      std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
      std::env::temp_dir(),
    ];

    let mut chosen: Option<PathBuf> = None;
    for dir in candidates {
      let path = dir.join("launcher.log");
      if std::fs::create_dir_all(&dir).is_err() {
        continue;
      }
      // Rotation: start each run with a fresh log.
      let _ = std::fs::remove_file(&path);
      if OpenOptions::new().create(true).write(true).append(true).open(&path).is_ok() {
        chosen = Some(path);
        break;
      }
    }

    if chosen.is_none() {
      eprintln!("Logger: cannot open launcher.log in CWD or temp dir — console-only logging");
    }

    Logger { log_file_path: chosen, min_level }
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

    print!("{}", &line);

    let Some(log_file_path) = &self.log_file_path else {
      return;
    };

    // Открываем, пишем, закрываем — как вы просили
    // A locked/read-only log file must degrade to stderr instead of
    // panicking inside log::Log (which would poison the logger mutex and
    // kill every subsequent log call).
    let mut file = match OpenOptions::new().write(true).append(true).open(log_file_path) {
      Ok(file) => file,
      Err(e) => {
        eprintln!("Failed to open log file {:?}: {}", log_file_path, e);
        return;
      }
    };

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
  pub fn log_path(&self) -> Option<&Path> {
    self.log_file_path.as_deref()
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
