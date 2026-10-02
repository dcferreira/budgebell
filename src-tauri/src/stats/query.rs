//! The shared date-ranged day-log query (design spec §6.1): resolves a
//! rollover-day's real time bounds from the day config, fetches its events
//! joined with habit details from the store, and folds in the day summary
//! and longest sedentary gap. This is the one impure edge in the stats data
//! path — reused as-is by the `day_log` Tauri command and the MCP `day_log`
//! tool, so the resolution logic exists exactly once.
//!
//! The day window (design spec §3.9) is a per-rotation scheduling default
//! only, so it plays no part here: the longest gap is computed purely from
//! actual movements within the rollover-day, with `now` as the trailing edge
//! only when `date` is today (design spec §3.9).

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Serialize;

use crate::clock::Zone;
use crate::domain::DayConfig;
use crate::quiet_os::CalendarEvent;
use crate::scheduler::{rollover_day, rollover_day_start};
use crate::store::{LoggedEvent, Store, StoreError};

use super::{day_summary, longest_sedentary_gap, DaySummary, SedentaryGap};

/// A calendar event shown as a context row in the Stats window's activity
/// list (design spec §3.9). It never affects the summary counts or the
/// longest-sedentary-gap — it only lets the user see their day in context and
/// verify the calendar detector. `start`/`end` are unix epoch seconds (UTC),
/// like [`LoggedEvent`]'s `at`, so meetings interleave with movements on one
/// timeline. `is_call` mirrors the
/// with-others rule (at least one other attendee).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Meeting {
    pub title: String,
    pub start: i64,
    pub end: i64,
    pub attendee_count: u32,
    pub is_call: bool,
}

impl From<&CalendarEvent> for Meeting {
    fn from(event: &CalendarEvent) -> Self {
        Self {
            title: event.title.clone(),
            start: event.start.timestamp(),
            end: event.end.timestamp(),
            attendee_count: event.other_attendee_count,
            is_call: event.other_attendee_count >= 1,
        }
    }
}

/// The real instants bounding `date`'s rollover-day (design spec §4.4):
/// `[rollover on date, rollover on date+1)`, each rollover read in `zone` —
/// so the day spans 23 or 25 hours across a DST change. Shared by the store
/// query here and the calendar day-listing at the command edge, so both agree
/// on exactly which window a "day" spans.
pub fn rollover_day_bounds(
    day_config: DayConfig,
    date: NaiveDate,
    zone: Zone,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let day_start = rollover_day_start(date, day_config.rollover, zone);
    let day_end = rollover_day_start(date + Duration::days(1), day_config.rollover, zone);
    (day_start, day_end)
}

/// The date-ranged day-log payload (design spec §3.9/§6.1): `date`'s
/// rollover-day events, each already joined with its habit's name and
/// category, plus the shared aggregations — so a caller (the Stats window,
/// or an MCP tool) holds no logic of its own beyond rendering this.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayLog {
    pub date: NaiveDate,
    pub events: Vec<LoggedEvent>,
    pub summary: DaySummary,
    pub longest_gap: Option<SedentaryGap>,
    /// The day's calendar events shown as context rows (design spec §3.9),
    /// filtered per the calendar config. Populated at the command edge (the
    /// impure OS probe), so the pure store query leaves it empty. Never feeds
    /// the summary or the longest-gap.
    pub meetings: Vec<Meeting>,
}

/// Fetches and aggregates `date`'s rollover-day (design spec §4.4 — bounded
/// by the day rollover, not midnight). `now` resolves the longest-gap's
/// trailing edge: `now` itself when `date` is `now`'s own rollover-day,
/// otherwise the gap is computed from movements alone (design spec §3.9).
/// `zone` is the local zone the rollover is read in.
pub fn day_log(
    store: &Store,
    day_config: DayConfig,
    date: NaiveDate,
    now: DateTime<Utc>,
    zone: Zone,
) -> Result<DayLog, StoreError> {
    let (day_start, day_end) = rollover_day_bounds(day_config, date, zone);

    let events = store.list_events_between(day_start.timestamp(), day_end.timestamp())?;
    let bare_events: Vec<_> = events.iter().map(|logged| logged.event.clone()).collect();

    let summary = day_summary(&bare_events);
    let is_today = date == rollover_day(now, day_config.rollover, zone);
    let trailing_now = is_today.then(|| now.timestamp());
    let longest_gap = longest_sedentary_gap(&bare_events, trailing_now);

    Ok(DayLog {
        date,
        events,
        summary,
        longest_gap,
        // The store query is pure; the day's meetings are read from the OS
        // calendar at the command edge and attached there.
        meetings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{london as dt, LONDON};
    use crate::domain::TimeOfDay;
    use crate::store::{Category, EventAction, NewEvent, NewHabit, TriggerKind};

    fn default_day_config() -> DayConfig {
        DayConfig {
            rollover: TimeOfDay::new(4, 0).expect("valid time"),
            day_window: crate::domain::TimeWindow::new(
                TimeOfDay::new(9, 0).expect("valid time"),
                TimeOfDay::new(18, 0).expect("valid time"),
            )
            .expect("valid window"),
        }
    }

    fn store_with_habit() -> (Store, i64) {
        let store = Store::open_in_memory().expect("in-memory store opens");
        let habit_id = store
            .insert_habit(&NewHabit {
                name: "Lunge-and-reach".to_string(),
                instructions: "5 slow reps/leg, reach overhead".to_string(),
                media_path: None,
                category: Category::Exercise,
                enabled: true,
                trigger_kind: TriggerKind::RotationMember,
                trigger_config_json: "{}".to_string(),
                weight: Some(2),
                rotation_id: None,
                created_at: 0,
            })
            .expect("habit insert succeeds");
        (store, habit_id)
    }

    #[test]
    fn day_log_only_includes_events_within_the_rollover_day_not_the_calendar_day() {
        // Given events either side of a 04:00 rollover on 2026-07-21: one at
        // 02:00 (still "yesterday" for scheduling purposes) and one at 10:00
        let (store, habit_id) = store_with_habit();
        store
            .append_event(&NewEvent {
                habit_id,
                action: EventAction::Done,
                at: dt(2026, 7, 21, 2, 0).timestamp(),
                shown_at: None,
            })
            .expect("append succeeds");
        store
            .append_event(&NewEvent {
                habit_id,
                action: EventAction::Done,
                at: dt(2026, 7, 21, 10, 0).timestamp(),
                shown_at: None,
            })
            .expect("append succeeds");

        // When fetching the 2026-07-21 rollover-day's log
        let log = day_log(
            &store,
            default_day_config(),
            NaiveDate::from_ymd_opt(2026, 7, 21).expect("valid date"),
            dt(2026, 7, 21, 12, 0),
            LONDON,
        )
        .expect("query succeeds");

        // Then only the 10:00 event belongs to this rollover-day — 02:00
        // belongs to the previous one
        assert_eq!(log.events.len(), 1);
        assert_eq!(log.events[0].event.at, dt(2026, 7, 21, 10, 0).timestamp());

        // And the previous rollover-day's log picks up the 02:00 event instead
        let previous_log = day_log(
            &store,
            default_day_config(),
            NaiveDate::from_ymd_opt(2026, 7, 20).expect("valid date"),
            dt(2026, 7, 21, 12, 0),
            LONDON,
        )
        .expect("query succeeds");
        assert_eq!(previous_log.events.len(), 1);
        assert_eq!(
            previous_log.events[0].event.at,
            dt(2026, 7, 21, 2, 0).timestamp()
        );
    }

    #[test]
    fn todays_single_movement_gaps_through_to_now_not_a_fabricated_window_edge() {
        // Given today's rollover-day with one movement at 10:00, and `now` at 14:00
        let (store, habit_id) = store_with_habit();
        store
            .append_event(&NewEvent {
                habit_id,
                action: EventAction::Done,
                at: dt(2026, 7, 21, 10, 0).timestamp(),
                shown_at: None,
            })
            .expect("append succeeds");

        // When fetching today's log at 14:00
        let log = day_log(
            &store,
            default_day_config(),
            NaiveDate::from_ymd_opt(2026, 7, 21).expect("valid date"),
            dt(2026, 7, 21, 14, 0),
            LONDON,
        )
        .expect("query succeeds");

        // Then the longest gap runs from that real movement (10:00) through
        // to `now` (14:00) — never from the 09:00 day-window start
        assert_eq!(
            log.longest_gap,
            Some(SedentaryGap {
                duration_secs: dt(2026, 7, 21, 14, 0).timestamp()
                    - dt(2026, 7, 21, 10, 0).timestamp(),
                start: dt(2026, 7, 21, 10, 0).timestamp(),
                end: dt(2026, 7, 21, 14, 0).timestamp(),
            })
        );
    }

    #[test]
    fn a_past_days_single_movement_has_no_meaningful_gap() {
        // Given a past rollover-day with only one movement, viewed from a
        // later `now` — a single movement can't form a gap on its own, and a
        // past day gets no trailing "now" edge
        let (store, habit_id) = store_with_habit();
        store
            .append_event(&NewEvent {
                habit_id,
                action: EventAction::Done,
                at: dt(2026, 7, 20, 10, 0).timestamp(),
                shown_at: None,
            })
            .expect("append succeeds");

        // When fetching 2026-07-20's log from 2026-07-21
        let log = day_log(
            &store,
            default_day_config(),
            NaiveDate::from_ymd_opt(2026, 7, 20).expect("valid date"),
            dt(2026, 7, 21, 14, 0),
            LONDON,
        )
        .expect("query succeeds");

        // Then there's no fabricated gap from the configured window end
        assert_eq!(log.longest_gap, None);
    }

    #[test]
    fn a_past_days_gap_is_only_between_its_two_real_movements() {
        // Given a past rollover-day with two movements
        let (store, habit_id) = store_with_habit();
        store
            .append_event(&NewEvent {
                habit_id,
                action: EventAction::Done,
                at: dt(2026, 7, 20, 10, 0).timestamp(),
                shown_at: None,
            })
            .expect("append succeeds");
        store
            .append_event(&NewEvent {
                habit_id,
                action: EventAction::Done,
                at: dt(2026, 7, 20, 12, 30).timestamp(),
                shown_at: None,
            })
            .expect("append succeeds");

        // When fetching 2026-07-20's log from a later day
        let log = day_log(
            &store,
            default_day_config(),
            NaiveDate::from_ymd_opt(2026, 7, 20).expect("valid date"),
            dt(2026, 7, 21, 14, 0),
            LONDON,
        )
        .expect("query succeeds");

        // Then the gap spans exactly the two real movements — no window edges
        assert_eq!(
            log.longest_gap,
            Some(SedentaryGap {
                duration_secs: dt(2026, 7, 20, 12, 30).timestamp()
                    - dt(2026, 7, 20, 10, 0).timestamp(),
                start: dt(2026, 7, 20, 10, 0).timestamp(),
                end: dt(2026, 7, 20, 12, 30).timestamp(),
            })
        );
    }

    #[test]
    fn a_day_with_no_events_summarises_to_zero_and_has_no_meaningful_gap() {
        // Given a rollover-day with nothing logged
        let (store, _habit_id) = store_with_habit();

        // When fetching that day's log
        let log = day_log(
            &store,
            default_day_config(),
            NaiveDate::from_ymd_opt(2026, 7, 20).expect("valid date"),
            dt(2026, 7, 21, 14, 0),
            LONDON,
        )
        .expect("query succeeds");

        // Then the summary is all zero and there's no fabricated gap
        assert_eq!(log.summary.done_count, 0);
        assert_eq!(log.longest_gap, None);
    }

    #[test]
    fn the_fall_back_rollover_day_spans_25_hours_and_holds_both_passes_of_the_repeated_hour() {
        // Given movements at 01:30 BST (00:30Z) and, an hour later, 01:30 GMT
        // (01:30Z) on 2026-10-25, when London's clocks go back
        let (store, habit_id) = store_with_habit();
        let first_pass = dt(2026, 10, 25, 1, 30);
        let second_pass = first_pass + Duration::hours(1);
        for at in [first_pass, second_pass] {
            store
                .append_event(&NewEvent {
                    habit_id,
                    action: EventAction::Done,
                    at: at.timestamp(),
                    shown_at: None,
                })
                .expect("append succeeds");
        }
        let date = NaiveDate::from_ymd_opt(2026, 10, 24).expect("valid date");

        // When resolving rollover-day 2026-10-24's bounds and fetching its log
        let (day_start, day_end) = rollover_day_bounds(default_day_config(), date, LONDON);
        let log = day_log(
            &store,
            default_day_config(),
            date,
            dt(2026, 10, 26, 12, 0),
            LONDON,
        )
        .expect("query succeeds");

        // Then the day runs 04:00 BST to 04:00 GMT — 25 real hours — and both
        // movements belong to it, an hour apart
        assert_eq!(day_end - day_start, Duration::hours(25));
        assert_eq!(log.events.len(), 2);
        assert_eq!(log.longest_gap.map(|gap| gap.duration_secs), Some(3_600));
    }

    #[test]
    fn the_spring_forward_rollover_day_spans_23_hours() {
        // Given rollover-day 2026-03-28, across which London's clocks go
        // forward an hour (01:00 GMT -> 02:00 BST on the 29th)
        let date = NaiveDate::from_ymd_opt(2026, 3, 28).expect("valid date");

        // When resolving its bounds
        let (day_start, day_end) = rollover_day_bounds(default_day_config(), date, LONDON);

        // Then it runs 04:00 GMT to 04:00 BST — 23 real hours
        assert_eq!(day_start, dt(2026, 3, 28, 4, 0));
        assert_eq!(day_end, dt(2026, 3, 29, 4, 0));
        assert_eq!(day_end - day_start, Duration::hours(23));
    }
}
