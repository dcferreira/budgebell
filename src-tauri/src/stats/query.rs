//! The shared date-ranged day-log query (design spec §6.1): resolves a
//! rollover-day's real time bounds from the day config, fetches its events
//! joined with habit details from the store, and folds in the day summary
//! and longest sedentary gap. This is the one impure edge in the stats data
//! path — reused as-is by the `day_log` Tauri command and the MCP `day_log`
//! tool, so the resolution logic exists exactly once.

use chrono::{Duration, NaiveDate, NaiveDateTime};
use serde::Serialize;

use crate::domain::DayConfig;
use crate::scheduler::{rollover_day, to_naive_time};
use crate::store::{LoggedEvent, Store, StoreError};

use super::{day_summary, longest_sedentary_gap, DaySummary, SedentaryGap};

/// The date-ranged day-log payload (design spec §3.9/§6.1): `date`'s
/// rollover-day events, each already joined with its habit's name and
/// category, plus the shared aggregations — so a caller (the Stats window,
/// or an MCP tool) holds no logic of its own beyond rendering this.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayLog {
    pub date: NaiveDate,
    pub events: Vec<LoggedEvent>,
    pub summary: DaySummary,
    pub longest_gap: SedentaryGap,
}

/// Fetches and aggregates `date`'s rollover-day (design spec §4.4 — bounded
/// by the day rollover, not midnight). `now` resolves the longest-gap's
/// right-hand edge: `now` itself for `date`'s own rollover-day, the day
/// window's configured end for a past day (design spec §3.9).
pub fn day_log(
    store: &Store,
    day_config: DayConfig,
    date: NaiveDate,
    now: NaiveDateTime,
) -> Result<DayLog, StoreError> {
    let rollover_time = to_naive_time(day_config.rollover);
    let day_start = date.and_time(rollover_time);
    let day_end = (date + Duration::days(1)).and_time(rollover_time);

    let events = store.list_events_between(
        day_start.and_utc().timestamp(),
        day_end.and_utc().timestamp(),
    )?;
    let bare_events: Vec<_> = events.iter().map(|logged| logged.event.clone()).collect();

    let summary = day_summary(&bare_events);
    let window_start = active_window_start(date, day_config);
    let window_end = active_window_end(date, day_config, now);
    let longest_gap = longest_sedentary_gap(
        &bare_events,
        window_start.and_utc().timestamp(),
        window_end.and_utc().timestamp(),
    );

    Ok(DayLog {
        date,
        events,
        summary,
        longest_gap,
    })
}

/// The active day window's real start instant on `date` (design spec §3.9).
fn active_window_start(date: NaiveDate, day_config: DayConfig) -> NaiveDateTime {
    date.and_time(to_naive_time(day_config.day_window.start))
}

/// The active day window's right-hand edge: `now` for `date`'s own
/// rollover-day, the window's configured end for a past day (design spec
/// §3.9). Clamped to never precede the window's start, so a stats request
/// made before the window opens today doesn't yield a negative-length gap.
fn active_window_end(date: NaiveDate, day_config: DayConfig, now: NaiveDateTime) -> NaiveDateTime {
    let start = active_window_start(date, day_config);
    let is_today = date == rollover_day(now, day_config.rollover);
    if is_today {
        return now.max(start);
    }

    let configured_end = if day_config.day_window.start <= day_config.day_window.end {
        date.and_time(to_naive_time(day_config.day_window.end))
    } else {
        (date + Duration::days(1)).and_time(to_naive_time(day_config.day_window.end))
    };
    configured_end.max(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::TimeOfDay;
    use crate::store::{Category, EventAction, NewEvent, NewHabit, TriggerKind};

    fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .expect("valid date")
            .and_hms_opt(hour, minute, 0)
            .expect("valid time")
    }

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
                at: dt(2026, 7, 21, 2, 0).and_utc().timestamp(),
                shown_at: None,
            })
            .expect("append succeeds");
        store
            .append_event(&NewEvent {
                habit_id,
                action: EventAction::Done,
                at: dt(2026, 7, 21, 10, 0).and_utc().timestamp(),
                shown_at: None,
            })
            .expect("append succeeds");

        // When fetching the 2026-07-21 rollover-day's log
        let log = day_log(
            &store,
            default_day_config(),
            NaiveDate::from_ymd_opt(2026, 7, 21).expect("valid date"),
            dt(2026, 7, 21, 12, 0),
        )
        .expect("query succeeds");

        // Then only the 10:00 event belongs to this rollover-day — 02:00
        // belongs to the previous one
        assert_eq!(log.events.len(), 1);
        assert_eq!(log.events[0].event.at, dt(2026, 7, 21, 10, 0).and_utc().timestamp());

        // And the previous rollover-day's log picks up the 02:00 event instead
        let previous_log = day_log(
            &store,
            default_day_config(),
            NaiveDate::from_ymd_opt(2026, 7, 20).expect("valid date"),
            dt(2026, 7, 21, 12, 0),
        )
        .expect("query succeeds");
        assert_eq!(previous_log.events.len(), 1);
        assert_eq!(
            previous_log.events[0].event.at,
            dt(2026, 7, 21, 2, 0).and_utc().timestamp()
        );
    }

    #[test]
    fn todays_longest_gap_uses_now_as_its_right_hand_edge() {
        // Given today's rollover-day with one movement at 10:00, and `now` at 14:00
        let (store, habit_id) = store_with_habit();
        store
            .append_event(&NewEvent {
                habit_id,
                action: EventAction::Done,
                at: dt(2026, 7, 21, 10, 0).and_utc().timestamp(),
                shown_at: None,
            })
            .expect("append succeeds");

        // When fetching today's log at 14:00
        let log = day_log(
            &store,
            default_day_config(),
            NaiveDate::from_ymd_opt(2026, 7, 21).expect("valid date"),
            dt(2026, 7, 21, 14, 0),
        )
        .expect("query succeeds");

        // Then the longest gap's right edge is `now` (14:00), not the
        // configured window end (18:00)
        assert_eq!(
            log.longest_gap.end,
            dt(2026, 7, 21, 14, 0).and_utc().timestamp()
        );
    }

    #[test]
    fn a_past_days_longest_gap_uses_the_configured_window_end() {
        // Given a past rollover-day with one movement at 10:00, viewed from
        // a later `now`
        let (store, habit_id) = store_with_habit();
        store
            .append_event(&NewEvent {
                habit_id,
                action: EventAction::Done,
                at: dt(2026, 7, 20, 10, 0).and_utc().timestamp(),
                shown_at: None,
            })
            .expect("append succeeds");

        // When fetching 2026-07-20's log from 2026-07-21
        let log = day_log(
            &store,
            default_day_config(),
            NaiveDate::from_ymd_opt(2026, 7, 20).expect("valid date"),
            dt(2026, 7, 21, 14, 0),
        )
        .expect("query succeeds");

        // Then the longest gap's right edge is the configured window end
        // (18:00 on 2026-07-20), not `now`
        assert_eq!(
            log.longest_gap.end,
            dt(2026, 7, 20, 18, 0).and_utc().timestamp()
        );
    }

    #[test]
    fn a_day_with_no_events_summarises_to_zero_and_the_gap_spans_the_whole_window() {
        // Given a rollover-day with nothing logged
        let (store, _habit_id) = store_with_habit();

        // When fetching that day's log
        let log = day_log(
            &store,
            default_day_config(),
            NaiveDate::from_ymd_opt(2026, 7, 20).expect("valid date"),
            dt(2026, 7, 21, 14, 0),
        )
        .expect("query succeeds");

        // Then the summary is all zero and the gap spans the full 09:00-18:00 window
        assert_eq!(log.summary.done_count, 0);
        assert_eq!(log.longest_gap.duration_secs, 9 * 3_600);
    }
}
