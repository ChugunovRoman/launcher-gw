use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

/// Phase of a startup sub-task.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "status", content = "detail", rename_all = "snake_case")]
pub enum Phase {
  Pending,
  Ok,
  Error(String),
}

/// Aggregate state of all startup sub-tasks.
/// The frontend reads this to show connection status and stale-data hints.
#[derive(Debug, Clone, Serialize)]
pub struct StartupState {
  pub providers: Phase,
  pub releases: Phase,
  pub user_data: Phase,
  pub profiles: Phase,
}

impl Default for StartupState {
  fn default() -> Self {
    Self {
      providers: Phase::Pending,
      releases: Phase::Pending,
      user_data: Phase::Pending,
      profiles: Phase::Pending,
    }
  }
}

/// Thread-safe tracker that emits `startup-state` events on every mutation.
pub struct StartupTracker {
  state: Mutex<StartupState>,
  app: AppHandle,
}

impl StartupTracker {
  pub fn new(app: AppHandle) -> Self {
    Self {
      state: Mutex::new(StartupState::default()),
      app,
    }
  }

  /// Mutate the state under a lock and emit the full snapshot as a Tauri event.
  pub async fn set(&self, f: impl FnOnce(&mut StartupState)) {
    let snapshot = {
      let mut guard = self.state.lock().await;
      f(&mut guard);
      guard.clone()
    };
    let _ = self.app.emit("startup-state", &snapshot);
  }

  /// Return a clone of the current state (for the `get_startup_state` command).
  pub async fn snapshot(&self) -> StartupState {
    self.state.lock().await.clone()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn phase_pending_serializes_without_detail() {
    let json = serde_json::to_value(Phase::Pending).unwrap();
    assert_eq!(json, serde_json::json!({"status": "pending"}));
  }

  #[test]
  fn phase_ok_serializes_without_detail() {
    let json = serde_json::to_value(Phase::Ok).unwrap();
    assert_eq!(json, serde_json::json!({"status": "ok"}));
  }

  #[test]
  fn phase_error_serializes_with_detail() {
    let json = serde_json::to_value(Phase::Error("boom".into())).unwrap();
    assert_eq!(json, serde_json::json!({"status": "error", "detail": "boom"}));
  }

  #[test]
  fn startup_state_serializes_all_phases() {
    let state = StartupState {
      providers: Phase::Ok,
      releases: Phase::Pending,
      user_data: Phase::Error("offline".into()),
      profiles: Phase::Ok,
    };
    let json = serde_json::to_value(&state).unwrap();
    assert_eq!(
      json,
      serde_json::json!({
        "providers": {"status": "ok"},
        "releases": {"status": "pending"},
        "user_data": {"status": "error", "detail": "offline"},
        "profiles": {"status": "ok"},
      })
    );
  }

  #[test]
  fn startup_state_default_is_all_pending() {
    let state = StartupState::default();
    assert_eq!(state.providers, Phase::Pending);
    assert_eq!(state.releases, Phase::Pending);
    assert_eq!(state.user_data, Phase::Pending);
    assert_eq!(state.profiles, Phase::Pending);
  }
}
