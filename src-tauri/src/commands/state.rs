//! The Tauri-managed application state: the store connection, the in-memory
//! `SchedulerState` that `schedule()` needs remembered between calls, and the
//! manual pause instant. Pause is applied at this edge rather than folded
//! into the locked `QuietState` — it's the UX off-switch (design spec §3.7),
//! not one of the three always-on quiet sources (§4.5).

use std::sync::{Mutex, MutexGuard};

use chrono::NaiveDateTime;

use crate::scheduler::SchedulerState;
use crate::store::Store;

use super::error::CommandError;

pub struct AppStateInner {
    pub store: Store,
    pub scheduler_state: SchedulerState,
    pub paused_until: Option<NaiveDateTime>,
}

/// Wraps [`AppStateInner`] behind a mutex so Tauri commands — which only see
/// `&self` via `tauri::State`— can mutate it.
pub struct AppState(Mutex<AppStateInner>);

impl AppState {
    pub fn new(store: Store) -> Self {
        Self(Mutex::new(AppStateInner {
            store,
            scheduler_state: SchedulerState::default(),
            paused_until: None,
        }))
    }

    /// Locks the inner state, failing loudly rather than panicking if a
    /// prior panic poisoned the mutex.
    pub fn lock(&self) -> Result<MutexGuard<'_, AppStateInner>, CommandError> {
        self.0.lock().map_err(|_| CommandError::StatePoisoned)
    }
}
