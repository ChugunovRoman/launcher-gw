use log::{Level, Metadata, Record};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Keep at most this many launcher sessions in the log file.
const MAX_LOG_SESSIONS: usize = 50;
/// Maximum size in bytes for a single log session. When exceeded, further
/// writes for that session are dropped (after a single truncation marker) to
/// prevent the log from growing without bound during a long-running launcher
/// instance.
const MAX_SESSION_BYTES: u64 = 5 * 1024 * 1024; // 5 MB
const SESSION_SEPARATOR: &str = "========== Launcher started:";

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
  /// Bytes written to the current session; shared across clones.
  session_bytes: Arc<AtomicU64>,
  /// Set once the session size limit has been reported in the file, so the
  /// truncation marker is written exactly once.
  truncation_marked: Arc<AtomicBool>,
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
      // Append to existing log file (session rotation is handled separately).
      if OpenOptions::new().create(true).write(true).append(true).open(&path).is_ok() {
        chosen = Some(path);
        break;
      }
    }

    if chosen.is_none() {
      eprintln!("Logger: cannot open launcher.log in CWD or temp dir — console-only logging");
    }

    // Trim old sessions and write a new session header.
    if let Some(ref path) = chosen {
      Self::trim_old_sessions(path);
      Self::write_session_header(path);
    }

    Logger {
      log_file_path: chosen,
      min_level,
      session_bytes: Arc::new(AtomicU64::new(0)),
      truncation_marked: Arc::new(AtomicBool::new(false)),
    }
  }

  /// Remove the oldest session(s) when the log file exceeds MAX_LOG_SESSIONS.
  /// Keeps the last (MAX_LOG_SESSIONS - 1) sessions so that after appending
  /// the new session header the total is exactly MAX_LOG_SESSIONS.
  fn trim_old_sessions(path: &Path) {
    let Ok(content) = std::fs::read_to_string(path) else { return };

    let positions: Vec<usize> = content
      .match_indices(SESSION_SEPARATOR)
      .map(|(pos, _)| pos)
      .collect();

    if positions.len() < MAX_LOG_SESSIONS {
      return;
    }

    // Drop oldest sessions: keep the last (MAX_LOG_SESSIONS - 1) so that
    // after appending the new header the total is exactly MAX_LOG_SESSIONS.
    let keep = MAX_LOG_SESSIONS - 1;
    let trim_from = positions[positions.len() - keep];
    let trimmed = &content[trim_from..];

    // Atomic replace: write to a temp file then rename.
    let tmp_path = path.with_extension("log.tmp");
    if std::fs::write(&tmp_path, trimmed).is_err() {
      return;
    }
    let _ = std::fs::rename(&tmp_path, path);
  }

  /// Append a session header with the current timestamp.
  fn write_session_header(path: &Path) {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let header = format!("\n{} {} ==========\n", SESSION_SEPARATOR, timestamp);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
      let _ = write!(file, "{}", header);
    }
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

    // Enforce per-session size limit to prevent unbounded growth.
    let line_bytes = line.len() as u64;
    let current = self.session_bytes.fetch_add(line_bytes, Ordering::Relaxed);
    if current > MAX_SESSION_BYTES {
      // Write a marker on the first overflow only: a log that just stops in
      // the middle of a session (easy to hit with LogLevel::Debug) is
      // otherwise indistinguishable from a crash or a hang.
      if self.truncation_marked.swap(true, Ordering::Relaxed) {
        return;
      }

      let marker = format!(
        "[{}] WARN - log truncated: session limit of {} bytes reached, further entries are dropped\n",
        timestamp, MAX_SESSION_BYTES
      );
      eprint!("{}", &marker);
      if let Ok(mut file) = OpenOptions::new().create(true).write(true).append(true).open(log_file_path) {
        let _ = write!(file, "{}", marker);
      }

      return;
    }

    // Открываем, пишем, закрываем — как вы просили
    // A locked/read-only log file must degrade to stderr instead of
    // panicking inside log::Log (which would poison the logger mutex and
    // kill every subsequent log call).
    let mut file = match OpenOptions::new().create(true).write(true).append(true).open(log_file_path) {
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
    let msg = format!("{} - {}", record.target(), record.args());
    // `try_lock`, never `lock`: the panic hook logs through this very
    // non-reentrant std Mutex, so a panic raised while the guard is held (or
    // a poisoned mutex left behind by an earlier one) would hang the process
    // instead of reporting the panic. Degrade to stderr instead.
    match self.inner.try_lock() {
      Ok(logger) => match record.level() {
        Level::Error => logger.error(&msg),
        Level::Warn => logger.warn(&msg),
        Level::Info => logger.info(&msg),
        _ => logger.debug(&msg),
      },
      Err(_) => eprintln!("[{}] {}", record.level(), msg),
    }
  }

  fn flush(&self) {}
}
