//! The day summary tile (design spec §3.9/§6.1): counts of `done`/`skipped`
//! events, total measured movement time, and adherence — a pure fold over a
//! day's events, no clock or database involved.

use serde::Serialize;

use crate::store::{Event, EventAction};

/// The four-tile day summary (design spec §3.9). `adherence_pct` is `0.0`
/// when there were no `done`/`skipped` events at all, rather than an
/// undefined percentage.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct DaySummary {
    pub done_count: u32,
    pub skipped_count: u32,
    pub total_moving_secs: i64,
    pub adherence_pct: f64,
}

/// Folds a day's events into its summary tile. `total_moving_secs` sums each
/// `done` event's duration (design spec §3.8) — a `done` event with no
/// recorded `shown_at` (e.g. a pre-migration row) contributes to
/// `done_count` and `adherence_pct` but not to the moving-time total.
pub fn day_summary(events: &[Event]) -> DaySummary {
    let done_count = events
        .iter()
        .filter(|event| event.action == EventAction::Done)
        .count() as u32;
    let skipped_count = events
        .iter()
        .filter(|event| event.action == EventAction::Skipped)
        .count() as u32;
    let total_moving_secs = events.iter().filter_map(Event::done_duration_secs).sum();

    let acted = done_count + skipped_count;
    let adherence_pct = if acted == 0 {
        0.0
    } else {
        (done_count as f64 / acted as f64) * 100.0
    };

    DaySummary {
        done_count,
        skipped_count,
        total_moving_secs,
        adherence_pct,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(action: EventAction, at: i64, shown_at: Option<i64>) -> Event {
        Event {
            id: 1,
            habit_id: 1,
            action,
            at,
            shown_at,
        }
    }

    #[test]
    fn an_empty_day_summarises_to_all_zeros() {
        // Given a day with no events at all
        // When summarising it
        let summary = day_summary(&[]);

        // Then every tile is zero, and adherence is 0 rather than undefined
        assert_eq!(
            summary,
            DaySummary {
                done_count: 0,
                skipped_count: 0,
                total_moving_secs: 0,
                adherence_pct: 0.0,
            }
        );
    }

    #[test]
    fn a_day_of_only_skips_has_zero_adherence_and_zero_moving_time() {
        // Given two skipped events and no done events
        let events = vec![
            event(EventAction::Skipped, 100, None),
            event(EventAction::Skipped, 200, None),
        ];

        // When summarising the day
        let summary = day_summary(&events);

        // Then adherence is 0% — no drill was ever completed
        assert_eq!(summary.done_count, 0);
        assert_eq!(summary.skipped_count, 2);
        assert_eq!(summary.total_moving_secs, 0);
        assert_eq!(summary.adherence_pct, 0.0);
    }

    #[test]
    fn a_single_movement_contributes_its_duration_and_full_adherence() {
        // Given one done event, shown 108 seconds before it was actioned
        let events = vec![event(EventAction::Done, 1_700_000_108, Some(1_700_000_000))];

        // When summarising the day
        let summary = day_summary(&events);

        // Then it counts as one done, contributes its duration, and full adherence
        assert_eq!(summary.done_count, 1);
        assert_eq!(summary.skipped_count, 0);
        assert_eq!(summary.total_moving_secs, 108);
        assert_eq!(summary.adherence_pct, 100.0);
    }

    #[test]
    fn mixed_done_and_skipped_events_yield_the_expected_adherence_percentage() {
        // Given one done (300s) and one skipped event
        let events = vec![
            event(EventAction::Done, 1_000_300, Some(1_000_000)),
            event(EventAction::Skipped, 2_000_000, None),
        ];

        // When summarising the day
        let summary = day_summary(&events);

        // Then adherence is done / (done + skipped) = 50%
        assert_eq!(summary.done_count, 1);
        assert_eq!(summary.skipped_count, 1);
        assert_eq!(summary.total_moving_secs, 300);
        assert_eq!(summary.adherence_pct, 50.0);
    }

    #[test]
    fn multiple_done_events_sum_their_durations() {
        // Given three done events with distinct durations
        let events = vec![
            event(EventAction::Done, 1_100, Some(1_000)),
            event(EventAction::Done, 2_250, Some(2_000)),
            event(EventAction::Done, 3_500, Some(3_000)),
        ];

        // When summarising the day
        let summary = day_summary(&events);

        // Then the total moving time is the sum of every duration: 100+250+500
        assert_eq!(summary.total_moving_secs, 850);
    }

    #[test]
    fn a_done_event_without_a_shown_at_counts_towards_adherence_but_not_moving_time() {
        // Given a done event with no shown_at (e.g. a pre-migration row)
        let events = vec![event(EventAction::Done, 1_000, None)];

        // When summarising the day
        let summary = day_summary(&events);

        // Then it still counts as a completed drill, but contributes no
        // measurable duration
        assert_eq!(summary.done_count, 1);
        assert_eq!(summary.total_moving_secs, 0);
        assert_eq!(summary.adherence_pct, 100.0);
    }

    #[test]
    fn snoozed_and_expired_events_are_ignored_by_the_summary() {
        // Given only snoozed and expired events — neither is a completion or
        // a skip
        let events = vec![
            event(EventAction::Snoozed, 100, None),
            event(EventAction::Expired, 200, None),
        ];

        // When summarising the day
        let summary = day_summary(&events);

        // Then nothing is counted at all
        assert_eq!(summary.done_count, 0);
        assert_eq!(summary.skipped_count, 0);
        assert_eq!(summary.adherence_pct, 0.0);
    }
}
