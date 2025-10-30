use winit::dpi::PhysicalSize;
use winit::monitor::VideoMode;

// Вспомогательная функция для вычисления площади разрешения
fn resolution_area(size: PhysicalSize<u32>) -> u64 {
  (size.width as u64) * (size.height as u64)
}

// Сравнение двух видеорежимов: сначала по площади (убывание), потом по частоте (убывание)
fn compare_video_modes(a: &VideoMode, b: &VideoMode) -> std::cmp::Ordering {
  let area_a = resolution_area(a.size());
  let area_b = resolution_area(b.size());

  area_b
    .cmp(&area_a) // сначала по площади (больше → меньше)
    .then_with(|| b.refresh_rate_millihertz().cmp(&a.refresh_rate_millihertz())) // потом по частоте (выше → ниже)
}

/// Получает все разрешения основного монитора, отсортированные по убыванию качества
pub fn get_available_resolutions() -> Result<Vec<String>, String> {
  let event_loop = winit::event_loop::EventLoop::new().map_err(|e| format!("Не удалось создать EventLoop: {}", e))?;

  let primary_monitor = event_loop.primary_monitor().ok_or("Основной монитор не найден")?;

  let mut seen = std::collections::HashSet::new();
  let mut resolutions_with_data = Vec::new();

  for mode in primary_monitor.video_modes() {
    let size = mode.size();
    let refresh_rate = mode.refresh_rate_millihertz() / 1000;
    let key = (size.width, size.height, refresh_rate);

    if seen.insert(key) {
      resolutions_with_data.push((mode, format!("{}x{} ({}Hz)", size.width, size.height, refresh_rate)));
    }
  }

  if resolutions_with_data.is_empty() {
    return Err("Не удалось получить видеорежимы".to_string());
  }

  // Сортируем по качеству (площадь + частота)
  resolutions_with_data.sort_by(|(a, _), (b, _)| compare_video_modes(a, b));

  // Извлекаем только строки
  let resolutions: Vec<String> = resolutions_with_data.into_iter().map(|(_, s)| s).collect();
  Ok(resolutions)
}
