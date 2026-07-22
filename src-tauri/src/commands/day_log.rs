//! `day_log` — the date-ranged log Tauri command (design spec §6.1/§3.9):
//! resolves the app's day config, then delegates to the shared
//! `stats::day_log` query so the Stats window renders from a single
//! implementation shared with the MCP `day_log` tool.

use chrono::NaiveDate;
use tauri::State;

use crate::domain::DayConfig;
use crate::quiet_os;
use crate::stats::{self, DayLog, Meeting};

use super::state::AppState;
use super::CommandError;

fn now() -> chrono::NaiveDateTime {
    chrono::Local::now().naive_local()
}

/// The date-ranged log the Stats window (design spec §3.9) renders:
/// `date`'s rollover-day events (§4.4), each joined with its habit's name
/// and category, plus the shared day summary and longest-sedentary-gap
/// aggregation (§6.1) — the window holds no logic of its own beyond
/// rendering this.
#[tauri::command]
pub fn day_log(state: State<AppState>, date: NaiveDate) -> Result<DayLog, CommandError> {
    let inner = state.lock()?;
    let config = inner
        .store
        .read_config()?
        .ok_or(CommandError::ConfigNotSet)?;
    let day_config = DayConfig::try_from(&config)?;
    let mut log = stats::day_log(&inner.store, day_config, date, now())?;

    // Overlay the day's calendar events as context rows (design spec §3.9),
    // filtered to mirror exactly what the meeting-pause rule considers. This
    // is the impure OS edge the pure store query deliberately leaves out.
    let (day_start, day_end) = stats::rollover_day_bounds(day_config, date);
    let events = quiet_os::list_day_meetings(
        day_start,
        day_end,
        config.calendar_mode,
        config.calendar_pause_enabled,
    )?;
    log.meetings = events.iter().map(Meeting::from).collect();

    Ok(log)
}
