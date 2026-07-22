//! `Decision` (design spec §4.6): `schedule()`'s pure output.

use chrono::NaiveDateTime;

use crate::domain::Habit;

use super::ids::HabitId;

/// A habit due right now, per `schedule()`'s decision.
#[derive(Debug, Clone, PartialEq)]
pub struct DueHabit {
    pub habit_id: HabitId,
    pub habit: Habit,
}

/// A scheduled habit whose day has rolled over without being completed
/// (design spec §4.2/§4.7 example C) — the caller logs this as an `expired`
/// event. Not carried forward: the next day's occurrence is a fresh one.
#[derive(Debug, Clone, PartialEq)]
pub struct Expiration {
    pub habit_id: HabitId,
    pub habit: Habit,
}

/// The pure scheduler's output (design spec §4.6): which habit (if any) is
/// due right now, when to next re-invoke `schedule()`, and which scheduled
/// habits have expired unactioned since the last rollover.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Decision {
    pub due_now: Option<DueHabit>,
    pub next_due: Option<NaiveDateTime>,
    pub expirations: Vec<Expiration>,
}
