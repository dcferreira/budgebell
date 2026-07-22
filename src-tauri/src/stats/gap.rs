//! The longest sedentary gap (design spec §3.9/§6.1): the signature stat —
//! the largest gap between movements, with the time window it spanned. A
//! pure fold over a day's `done` events plus the active window's bounds; no
//! clock, no database.

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

/// Computes the longest gap between consecutive `done` events within the
/// half-open active window `[window_start, window_end)`, considering the
/// edges: `window_start -> first movement` and `last movement -> window_end`
/// (design spec §3.9). With no `done` events in the window, the gap spans
/// the window in full.
///
/// Callers resolve `window_end` themselves: `now` for today, the day
/// window's configured end for a past day.
pub fn longest_sedentary_gap(events: &[Event], window_start: i64, window_end: i64) -> SedentaryGap {
    let mut done_times: Vec<i64> = events
        .iter()
        .filter(|event| event.action == EventAction::Done)
        .map(|event| event.at)
        .filter(|&at| at >= window_start && at < window_end)
        .collect();
    done_times.sort_unstable();

    let mut boundaries = Vec::with_capacity(done_times.len() + 2);
    boundaries.push(window_start);
    boundaries.extend(done_times);
    boundaries.push(window_end);

    boundaries
        .windows(2)
        .map(|pair| SedentaryGap {
            duration_secs: pair[1] - pair[0],
            start: pair[0],
            end: pair[1],
        })
        .max_by_key(|gap| gap.duration_secs)
        .expect("boundaries always has at least the window's own start and end")
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
    fn with_no_done_events_the_gap_spans_the_whole_active_window() {
        // Given a day with no done events at all (design spec §3.9)
        let events: Vec<Event> = vec![];

        // When computing the longest sedentary gap over a 09:00-18:00 window
        let gap = longest_sedentary_gap(&events, 9 * 3_600, 18 * 3_600);

        // Then it spans the entire window
        assert_eq!(
            gap,
            SedentaryGap {
                duration_secs: 9 * 3_600,
                start: 9 * 3_600,
                end: 18 * 3_600,
            }
        );
    }

    #[test]
    fn skipped_events_do_not_count_as_movements() {
        // Given only a skipped event inside the window
        let events = vec![skipped_at(12 * 3_600)];

        // When computing the longest gap
        let gap = longest_sedentary_gap(&events, 9 * 3_600, 18 * 3_600);

        // Then it still spans the whole window — a skip is not a movement
        assert_eq!(gap.duration_secs, 9 * 3_600);
    }

    #[test]
    fn a_single_movement_splits_the_window_into_two_candidate_gaps() {
        // Given one done event at 10:00, inside a 09:00-18:00 window
        let events = vec![done_at(10 * 3_600)];

        // When computing the longest gap
        let gap = longest_sedentary_gap(&events, 9 * 3_600, 18 * 3_600);

        // Then the larger of the two candidate gaps wins: 10:00->18:00 (8h)
        // beats 09:00->10:00 (1h)
        assert_eq!(
            gap,
            SedentaryGap {
                duration_secs: 8 * 3_600,
                start: 10 * 3_600,
                end: 18 * 3_600,
            }
        );
    }

    #[test]
    fn the_largest_gap_between_two_movements_wins_over_the_edges() {
        // Given movements shortly after the window opens and shortly before
        // it closes, with a long gap between them
        let events = vec![done_at(9 * 3_600 + 300), done_at(17 * 3_600 + 3_600 - 300)];

        // When computing the longest gap
        let gap = longest_sedentary_gap(&events, 9 * 3_600, 18 * 3_600);

        // Then the middle gap (between the two movements) is the longest,
        // not either edge
        assert_eq!(gap.start, 9 * 3_600 + 300);
        assert_eq!(gap.end, 17 * 3_600 + 3_600 - 300);
    }

    #[test]
    fn a_movement_exactly_at_the_window_start_is_included_and_yields_a_zero_length_first_gap() {
        // Given a movement exactly at the window's opening instant
        let events = vec![done_at(9 * 3_600)];

        // When computing the longest gap
        let gap = longest_sedentary_gap(&events, 9 * 3_600, 18 * 3_600);

        // Then the whole window-to-close span is the longest gap — the
        // window-start-to-first-movement span is zero
        assert_eq!(
            gap,
            SedentaryGap {
                duration_secs: 9 * 3_600,
                start: 9 * 3_600,
                end: 18 * 3_600,
            }
        );
    }

    #[test]
    fn a_movement_at_or_after_the_window_end_is_excluded() {
        // Given a movement exactly at, and one after, the window's close
        let events = vec![done_at(18 * 3_600), done_at(19 * 3_600)];

        // When computing the longest gap over 09:00-18:00
        let gap = longest_sedentary_gap(&events, 9 * 3_600, 18 * 3_600);

        // Then both are outside the window, so the gap spans it in full
        assert_eq!(gap.duration_secs, 9 * 3_600);
    }

    #[test]
    fn ties_between_candidate_gaps_resolve_deterministically() {
        // Given a movement exactly at the window's midpoint, splitting it
        // into two equal halves
        let events = vec![done_at(13 * 3_600 + 30 * 60)];

        // When computing the longest gap over a 09:00-18:00 window
        let first = longest_sedentary_gap(&events, 9 * 3_600, 18 * 3_600);
        let second = longest_sedentary_gap(&events, 9 * 3_600, 18 * 3_600);

        // Then the same input always yields the same result — deterministic,
        // not an arbitrary tie-break that could vary between calls
        assert_eq!(first, second);
    }
}
