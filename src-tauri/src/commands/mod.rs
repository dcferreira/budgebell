//! Tauri commands wiring the store + scheduler to the frontend (design spec
//! §10 "commands" task): `list_due`, complete/skip/snooze, pause/resume,
//! `list_habits`, `get_config`/`set_config`. Command bodies stay thin — all
//! actual logic lives in this module's sibling helpers, each of which is
//! testable against an in-memory `Store` without a running Tauri app.

mod actions;
mod build;
mod dto;
mod error;
mod handlers;
mod list_due;
mod pause_until;
mod state;

pub use dto::DueHabitDto;
pub use error::CommandError;
pub use handlers::{
    complete_habit, current_due, get_config, list_due, list_due_now, list_habits, pause, resume,
    set_config, skip_habit, snooze_habit,
};
pub use state::AppState;
