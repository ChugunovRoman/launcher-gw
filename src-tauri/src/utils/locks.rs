use std::sync::{Mutex, MutexGuard};

/// Lock a std::sync::Mutex, recovering from poisoning instead of panicking.
///
/// A panic in one task while holding one of the launcher's shared mutexes
/// (cancel maps, provider status/manifest, upload counters) must not brick
/// every other command with a cascade of unwrap-panics on the poisoned lock.
/// The data guarded by these mutexes is plain values/maps without
/// cross-field invariants, so taking the possibly half-updated value is
/// safer than a permanent panic loop until restart.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  match mutex.lock() {
    Ok(guard) => guard,
    Err(poisoned) => {
      log::warn!("Mutex was poisoned by a panicked thread; recovering");
      poisoned.into_inner()
    }
  }
}
