//! The Tauri-managed application state: the store connection, the in-memory
//! `SchedulerState` that `schedule()` needs remembered between calls, and the
//! manual pause instant. Pause is applied at this edge rather than folded
//! into the locked `QuietState` — it's the UX off-switch (design spec §3.7),
//! not one of the three always-on quiet sources (§4.5).

use std::sync::{Mutex, MutexGuard};

use chrono::NaiveDateTime;

use crate::scheduler::SchedulerState;
use crate::store::Store;

use super::dto::DueHabitDto;
use super::error::CommandError;

/// The runtime's current due occurrence (design spec §4.5): the habit most
/// recently surfaced by the scheduler tick, plus the instant its toast was
/// shown. `complete_habit`/`skip_habit` read `shown_at` to log a duration
/// (§3.8); the runtime clears this — logging nothing — when the occurrence
/// is discarded rather than acted on (idle-withdraw, §4.5).
#[derive(Debug, Clone, PartialEq)]
pub struct CurrentDue {
    pub due: DueHabitDto,
    pub shown_at: NaiveDateTime,
}

pub struct AppStateInner {
    pub store: Store,
    pub scheduler_state: SchedulerState,
    pub paused_until: Option<NaiveDateTime>,
    /// The habit most recently surfaced by the scheduler tick (the runtime
    /// bridge, design spec §10). Held so a freshly-opened toast window can
    /// fetch the current nudge via `current_due` even if it missed the push
    /// event.
    pub current_due: Option<CurrentDue>,
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
            current_due: None,
        }))
    }

    /// Locks the inner state, failing loudly rather than panicking if a
    /// prior panic poisoned the mutex.
    pub fn lock(&self) -> Result<MutexGuard<'_, AppStateInner>, CommandError> {
        self.0.lock().map_err(|_| CommandError::StatePoisoned)
    }
}
