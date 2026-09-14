//! The Tauri command surface (design spec §10 "commands" task): thin
//! wrappers that read the wall clock, borrow the managed `AppState`, and
//! delegate to this module's testable helpers. No scheduling or
//! store-shaping logic lives here.

use chrono::NaiveDateTime;
use tauri::State;

use crate::domain::TimeOfDay;
use crate::quiet_os::probe_quiet_state;
use crate::store::{Config, EventAction, Habit};

use super::actions::record_action;
use super::dto::{DecisionDto, DueHabitDto};
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

/// Which habit (if any) is due right now. The quiet state (idle / in-meeting /
/// DND) is read here from the live OS probes (design spec §8/§9.2), gated by
/// the config toggles, then injected into the pure scheduler — the scheduler
/// itself never touches the OS.
#[tauri::command]
pub fn list_due(state: State<AppState>) -> Result<DecisionDto, CommandError> {
    list_due_now(&state)
}

/// The shared body of [`list_due`], callable off the IPC boundary so the
/// runtime scheduler tick (the `seed-wire` bridge, design spec §10) reuses the
/// exact same quiet-gating and state-transition path the frontend does.
pub fn list_due_now(state: &AppState) -> Result<DecisionDto, CommandError> {
    let mut guard = state.lock()?;
    let inner: &mut AppStateInner = &mut guard;
    let paused_until = inner.paused_until;
    let moment = now();
    let config = inner
        .store
        .read_config()?
        .ok_or(CommandError::ConfigNotSet)?;
    let quiet_state = probe_quiet_state(&config, moment)?;
    list_due_impl(
        &inner.store,
        &mut inner.scheduler_state,
        moment,
        quiet_state,
        paused_until,
    )
}

/// The habit most recently surfaced by the scheduler tick, if any — fetched by
/// the toast window on open so it can render even if it missed the push event.
#[tauri::command]
pub fn current_due(state: State<AppState>) -> Result<Option<DueHabitDto>, CommandError> {
    Ok(state
        .lock()?
        .current_due
        .as_ref()
        .map(|due| due.due.clone()))
}

/// The current due occurrence's `shown_at`, if `habit_id` matches it —
/// stale or mismatched state (e.g. a different habit was surfaced since)
/// yields `None` rather than misattributing another occurrence's timing
/// (design spec §3.8/§4.5).
fn shown_at_for(inner: &AppStateInner, habit_id: i64) -> Option<NaiveDateTime> {
    inner
        .current_due
        .as_ref()
        .filter(|due| due.due.habit_id == habit_id)
        .map(|due| due.shown_at)
}

/// Marks a habit done.
#[tauri::command]
pub fn complete_habit(state: State<AppState>, habit_id: i64) -> Result<(), CommandError> {
    let mut guard = state.lock()?;
    let inner: &mut AppStateInner = &mut guard;
    let rollover = current_rollover(inner)?;
    let shown_at = shown_at_for(inner, habit_id);
    record_action(
        &inner.store,
        &mut inner.scheduler_state,
        habit_id,
        EventAction::Done,
        now(),
        rollover,
        shown_at,
    )
}

/// Marks a habit skipped.
#[tauri::command]
pub fn skip_habit(state: State<AppState>, habit_id: i64) -> Result<(), CommandError> {
    let mut guard = state.lock()?;
    let inner: &mut AppStateInner = &mut guard;
    let rollover = current_rollover(inner)?;
    let shown_at = shown_at_for(inner, habit_id);
    record_action(
        &inner.store,
        &mut inner.scheduler_state,
        habit_id,
        EventAction::Skipped,
        now(),
        rollover,
        shown_at,
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
        None,
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
