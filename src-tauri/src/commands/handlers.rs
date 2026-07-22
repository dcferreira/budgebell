//! The Tauri command surface (design spec §10 "commands" task): thin
//! wrappers that read the wall clock, borrow the managed `AppState`, and
//! delegate to this module's testable helpers. No scheduling or
//! store-shaping logic lives here.

use chrono::NaiveDateTime;
use tauri::State;

use crate::domain::{QuietState, TimeOfDay};
use crate::store::{Config, EventAction, Habit};

use super::actions::record_action;
use super::dto::DecisionDto;
use super::list_due::list_due_impl;
use super::pause_until::resolve_pause_until;
use super::state::{AppState, AppStateInner};
use super::CommandError;

fn now() -> NaiveDateTime {
    chrono::Local::now().naive_local()
}

/// The day rollover instant currently configured, needed to resolve
/// completions/expirations correctly (design spec §4.4).
fn current_rollover(inner: &AppStateInner) -> Result<TimeOfDay, CommandError> {
    let config = inner
        .store
        .read_config()?
        .ok_or(CommandError::ConfigNotSet)?;
    Ok(config.day_rollover.parse()?)
}

/// Which habit (if any) is due right now. `idle`/`in_meeting`/`dnd` are
/// supplied by the caller (design spec §4.5) — the OS probes that will feed
/// them in production land in a later task.
#[tauri::command]
pub fn list_due(
    state: State<AppState>,
    idle: bool,
    in_meeting: bool,
    dnd: bool,
) -> Result<DecisionDto, CommandError> {
    let mut guard = state.lock()?;
    let inner: &mut AppStateInner = &mut guard;
    let paused_until = inner.paused_until;
    let quiet_state = QuietState {
        idle,
        in_meeting,
        dnd,
    };
    list_due_impl(
        &inner.store,
        &mut inner.scheduler_state,
        now(),
        quiet_state,
        paused_until,
    )
}

/// Marks a habit done.
#[tauri::command]
pub fn complete_habit(state: State<AppState>, habit_id: i64) -> Result<(), CommandError> {
    let mut guard = state.lock()?;
    let inner: &mut AppStateInner = &mut guard;
    let rollover = current_rollover(inner)?;
    record_action(
        &inner.store,
        &mut inner.scheduler_state,
        habit_id,
        EventAction::Done,
        now(),
        rollover,
    )
}

/// Marks a habit skipped.
#[tauri::command]
pub fn skip_habit(state: State<AppState>, habit_id: i64) -> Result<(), CommandError> {
    let mut guard = state.lock()?;
    let inner: &mut AppStateInner = &mut guard;
    let rollover = current_rollover(inner)?;
    record_action(
        &inner.store,
        &mut inner.scheduler_state,
        habit_id,
        EventAction::Skipped,
        now(),
        rollover,
    )
}

/// Snoozes a habit — logged, but its scheduled fire is deliberately left
/// unresolved (see `commands::actions`).
#[tauri::command]
pub fn snooze_habit(state: State<AppState>, habit_id: i64) -> Result<(), CommandError> {
    let mut guard = state.lock()?;
    let inner: &mut AppStateInner = &mut guard;
    let rollover = current_rollover(inner)?;
    record_action(
        &inner.store,
        &mut inner.scheduler_state,
        habit_id,
        EventAction::Snoozed,
        now(),
        rollover,
    )
}

/// Pauses nudges for `duration_secs` seconds, or until an explicit instant —
/// exactly one of the two must be supplied (design spec §3.3/§3.4).
#[tauri::command]
pub fn pause(
    state: State<AppState>,
    duration_secs: Option<i64>,
    until: Option<NaiveDateTime>,
) -> Result<(), CommandError> {
    let mut inner = state.lock()?;
    inner.paused_until = Some(resolve_pause_until(now(), duration_secs, until)?);
    Ok(())
}

/// Resumes nudges immediately (design spec §3.5).
#[tauri::command]
pub fn resume(state: State<AppState>) -> Result<(), CommandError> {
    let mut inner = state.lock()?;
    inner.paused_until = None;
    Ok(())
}

/// Lists every habit, enabled or disabled.
#[tauri::command]
pub fn list_habits(state: State<AppState>) -> Result<Vec<Habit>, CommandError> {
    let inner = state.lock()?;
    Ok(inner.store.list_habits()?)
}

/// Reads the app-wide config, failing loudly if it hasn't been seeded yet
/// (seeding is the `seed-wire` task, design spec §10).
#[tauri::command]
pub fn get_config(state: State<AppState>) -> Result<Config, CommandError> {
    let inner = state.lock()?;
    inner.store.read_config()?.ok_or(CommandError::ConfigNotSet)
}

/// Writes the app-wide config, replacing any previous values.
#[tauri::command]
pub fn set_config(state: State<AppState>, config: Config) -> Result<(), CommandError> {
    let inner = state.lock()?;
    Ok(inner.store.write_config(&config)?)
}
