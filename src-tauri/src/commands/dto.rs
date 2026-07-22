//! IPC-shaped views onto the scheduler's pure output — the domain `Habit`
//! and scheduler `HabitId` aren't serialisable, so these flatten exactly the
//! fields the frontend needs.

use chrono::NaiveDateTime;
use serde::Serialize;

use crate::scheduler::DueHabit;
use crate::store::Category;

/// A habit due right now, shaped for IPC.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DueHabitDto {
    pub habit_id: i64,
    pub name: String,
    pub instructions: String,
    pub media_path: Option<String>,
    pub category: Category,
}

impl From<DueHabit> for DueHabitDto {
    fn from(due: DueHabit) -> Self {
        Self {
            habit_id: due.habit_id.0,
            name: due.habit.name,
            instructions: due.habit.instructions,
            media_path: due.habit.media_path,
            category: due.habit.category,
        }
    }
}

/// `schedule()`'s decision (design spec §4.6), shaped for IPC. Expirations
/// are applied and logged server-side (see `list_due`) rather than surfaced
/// to the frontend, which only needs to know what's due and when to poll next.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecisionDto {
    pub due_now: Option<DueHabitDto>,
    pub next_due: Option<NaiveDateTime>,
}
