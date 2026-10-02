//! IPC-shaped views onto the scheduler's pure output — the domain `Habit`
//! and scheduler `HabitId` aren't serialisable, so these flatten exactly the
//! fields the frontend needs.

use chrono::{DateTime, Utc};
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
    /// A UTC instant, serialised as RFC 3339 with a `Z` suffix.
    pub next_due: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_due_goes_over_ipc_as_an_rfc3339_utc_instant() {
        // Given a decision whose next check is 10:30 BST (09:30Z)
        let decision = DecisionDto {
            due_now: None,
            next_due: Some(crate::clock::london(2026, 7, 21, 10, 30)),
        };

        // When serialised for the frontend
        let json = serde_json::to_value(&decision).expect("serialises");

        // Then the instant carries its zone, so the webview can show it locally
        assert_eq!(json["next_due"], "2026-07-21T09:30:00Z");
    }

    #[test]
    fn a_pause_until_instant_must_carry_its_offset() {
        // Given the `pause` command's `until` argument as the frontend sends
        // it (`toISOString()`), and as a bare local time
        let iso: Result<DateTime<Utc>, _> = serde_json::from_str(r#""2026-07-21T09:30:00.000Z""#);
        let bare: Result<DateTime<Utc>, _> = serde_json::from_str(r#""2026-07-21T10:30:00""#);

        // Then the ISO instant is read as that UTC instant, and the zone-less
        // one is refused rather than guessed at
        assert_eq!(
            iso.expect("parses"),
            crate::clock::london(2026, 7, 21, 10, 30)
        );
        assert!(bare.is_err());
    }
}
