//! The longest sedentary gap (design spec §3.9/§6.1): the signature stat —
//! the largest gap between actual movements (consecutive `done` events)
//! within a day. A pure fold over a day's `done` events plus, only for
//! today, the trailing "last movement -> now" edge; no clock, no database.
//!
//! The day window (§3.9) is a per-rotation scheduling default only — it is
//! never treated as a fabricated leading or trailing edge here. A day with
//! fewer than the movements needed to form a gap has no meaningful sit to
//! report.

use serde::Serialize;

use crate::store::{Event, EventAction};

/// The longest sedentary gap: a duration plus the instants it spanned
/// (design spec §3.9), e.g. "2h 31m · 12:05–14:35".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SedentaryGap {
    pub duration_secs: i64,
    pub start: i64,
    pub end: i64,
}

/// Computes the longest gap between actual movements in `events` (design
/// spec §3.9): consecutive `done` events, plus — only when `trailing_now` is
/// `Some`, i.e. the requested day is today — the last movement's gap through
/// to `now`. Past days never get a fabricated trailing edge.
///
/// Returns `None` when there's no meaningful sit to report: zero movements,
/// or a single movement on a past day (no second movement to gap against,
/// and no "now" to gap through to).
pub fn longest_sedentary_gap(events: &[Event], trailing_now: Option<i64>) -> Option<SedentaryGap> {
    let mut done_times: Vec<i64> = events
        .iter()
        .filter(|event| event.action == EventAction::Done)
        .map(|event| event.at)
        .collect();
    done_times.sort_unstable();

    let mut candidates: Vec<SedentaryGap> = done_times
        .windows(2)
        .map(|pair| SedentaryGap {
            duration_secs: pair[1] - pair[0],
            start: pair[0],
            end: pair[1],
        })
        .collect();

    if let (Some(now), Some(&last)) = (trailing_now, done_times.last()) {
        candidates.push(SedentaryGap {
            duration_secs: now - last,
            start: last,
            end: now,
        });
    }

    candidates.into_iter().max_by_key(|gap| gap.duration_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn done_at(at: i64) -> Event {
        Event {
            id: 1,
            habit_id: 1,
            action: EventAction::Done,
            at,
            shown_at: None,
        }
    }

    fn skipped_at(at: i64) -> Event {
        Event {
            id: 2,
            habit_id: 1,
            action: EventAction::Skipped,
            at,
            shown_at: None,
        }
    }

    #[test]
    fn with_no_movements_at_all_there_is_no_meaningful_gap() {
        // Given a day with no done events at all, viewed as a past day
        let events: Vec<Event> = vec![];

        // When computing the longest sedentary gap
        let gap = longest_sedentary_gap(&events, None);

        // Then there's nothing to report — no fabricated window-edge gap
        assert_eq!(gap, None);
    }

    #[test]
    fn skipped_events_do_not_count_as_movements() {
        // Given only a skipped event, viewed as a past day
        let events = vec![skipped_at(12 * 3_600)];

        // When computing the longest gap
        let gap = longest_sedentary_gap(&events, None);

        // Then a skip is not a movement, so there's still nothing to report
        assert_eq!(gap, None);
    }

    #[test]
    fn a_single_movement_on_a_past_day_yields_no_gap() {
        // Given exactly one movement on a day that isn't today
        let events = vec![done_at(10 * 3_600)];

        // When computing the longest gap without a trailing "now"
        let gap = longest_sedentary_gap(&events, None);

        // Then one movement alone can't form a gap between movements
        assert_eq!(gap, None);
    }

    #[test]
    fn a_single_movement_on_today_gaps_through_to_now() {
        // Given exactly one movement at 10:00, and "now" at 14:00 (today)
        let events = vec![done_at(10 * 3_600)];

        // When computing the longest gap with "now" supplied
        let gap = longest_sedentary_gap(&events, Some(14 * 3_600));

        // Then the gap runs from that movement through to now
        assert_eq!(
            gap,
            Some(SedentaryGap {
                duration_secs: 4 * 3_600,
                start: 10 * 3_600,
                end: 14 * 3_600,
            })
        );
    }

    #[test]
    fn the_longest_gap_is_between_two_actual_movements() {
        // Given three movements, with the largest gap being the middle one
        let events = vec![done_at(9 * 3_600), done_at(11 * 3_600 + 28 * 60), done_at(17 * 3_600)];

        // When computing the longest gap on a past day
        let gap = longest_sedentary_gap(&events, None);

        // Then it's the gap between the second and third movements
        assert_eq!(
            gap,
            Some(SedentaryGap {
                duration_secs: 17 * 3_600 - (11 * 3_600 + 28 * 60),
                start: 11 * 3_600 + 28 * 60,
                end: 17 * 3_600,
            })
        );
    }

    #[test]
    fn todays_trailing_now_gap_can_win_over_gaps_between_movements() {
        // Given two movements close together, then a long gap through to now
        let events = vec![done_at(9 * 3_600), done_at(9 * 3_600 + 300)];

        // When computing the longest gap with "now" much later
        let gap = longest_sedentary_gap(&events, Some(14 * 3_600));

        // Then the trailing last-movement-to-now gap wins
        assert_eq!(
            gap,
            Some(SedentaryGap {
                duration_secs: 14 * 3_600 - (9 * 3_600 + 300),
                start: 9 * 3_600 + 300,
                end: 14 * 3_600,
            })
        );
    }

    #[test]
    fn a_past_days_gap_never_gets_a_trailing_now_edge() {
        // Given two movements on a past day
        let events = vec![done_at(9 * 3_600), done_at(9 * 3_600 + 300)];

        // When computing the longest gap without a trailing "now"
        let gap = longest_sedentary_gap(&events, None);

        // Then only the gap between the two movements is considered
        assert_eq!(
            gap,
            Some(SedentaryGap {
                duration_secs: 300,
                start: 9 * 3_600,
                end: 9 * 3_600 + 300,
            })
        );
    }

    #[test]
    fn ties_between_candidate_gaps_resolve_deterministically() {
        // Given three movements producing two equal-length gaps
        let events = vec![done_at(9 * 3_600), done_at(11 * 3_600), done_at(13 * 3_600)];

        // When computing the longest gap repeatedly
        let first = longest_sedentary_gap(&events, None);
        let second = longest_sedentary_gap(&events, None);

        // Then the same input always yields the same result
        assert_eq!(first, second);
    }
}
