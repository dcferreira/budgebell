//! `last_shown_state` (design spec §4.6): everything `schedule()` needs
//! remembered between calls. State transitions — recording what was shown,
//! marking done/skip/expired, resetting weekly counts — happen outside the
//! pure function, in the caller; this module only defines the shape.

use std::collections::HashMap;

use chrono::{NaiveDate, NaiveDateTime};

use super::ids::{HabitId, RotationId};

/// A rotation's most recent tick — which member fired and when. Used both to
/// compute the next tick (`last_shown_at + interval`) and to exclude that
/// member from the next draw (design spec §4.3: "MUST avoid showing the same
/// drill twice in a row").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RotationLastShown {
    pub habit_id: HabitId,
    pub at: NaiveDateTime,
}

/// One scheduled-habit occurrence that has fired but not yet been actioned
/// (design spec §4.2) — tracked so the scheduler doesn't re-fire it within
/// the same rollover-day, and so it can be reported as expired once the day
/// rolls over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScheduledFire {
    pub fired_at: NaiveDateTime,
    pub completed: bool,
}

/// Per-scheduled-habit persistent state (both at-time and weekly-count
/// triggers use this shape — weekly-count is a thin variant of at-time,
/// design spec §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScheduledHabitState {
    pub last_fire: Option<ScheduledFire>,
    /// Completions logged within `week_of`'s rollover-defined week. A stale
    /// `week_of` (not the current week) is treated as zero completions by
    /// the scheduler — the actual reset is a caller-side state transition
    /// once the caller re-derives this state for the new week.
    pub weekly_completions: u8,
    pub week_of: Option<NaiveDate>,
}

/// Everything `schedule()` needs to remember between calls, keyed by the ids
/// the caller assigns (design spec §4.6's `last_shown_state`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SchedulerState {
    pub rotations: HashMap<RotationId, RotationLastShown>,
    pub scheduled_habits: HashMap<HabitId, ScheduledHabitState>,
}
