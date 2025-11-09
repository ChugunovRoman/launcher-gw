use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::time;

// Тип для коллбэка: без параметров, может быть вызван из асинхронного контекста
type WakeCallback = Box<dyn Fn() + Send + Sync>;

#[derive(Clone)]
pub struct WakeDetector {
  callback: Arc<WakeCallback>,
}

impl WakeDetector {
  /// Создаёт детектор с указанным коллбэком
  pub fn new<F>(callback: F) -> Self
  where
    F: Fn() + Send + Sync + 'static,
  {
    Self {
      callback: Arc::new(Box::new(callback)),
    }
  }

  /// Запускает фоновый watcher в отдельной задаче Tokio
  pub fn start_watcher(self, timeout: f64) {
    tauri::async_runtime::spawn(async move {
      let mut last_system_time = SystemTime::now();
      let mut elapsed_time: f64 = 0.0;
      let mut detected = false;

      loop {
        time::sleep(Duration::from_secs(1)).await;

        let now_system = SystemTime::now();

        // Реальное системное время
        let elapsed_system: f64 = now_system.duration_since(last_system_time).unwrap_or(Duration::ZERO).as_secs_f64();

        // Если система "проспала" — реальное время ушло далеко вперёд
        if elapsed_system > 5.0 {
          log::info!("🖥️ Система вышла из сна! Пропущено ~{:.1} сек", elapsed_system);

          detected = true;
        }
        if detected {
          if elapsed_time >= timeout {
            detected = false;

            // Вызываем коллбэк
            (self.callback)();
          }

          elapsed_time += 1.0;
        }

        last_system_time = now_system;
      }
    });
  }
}
