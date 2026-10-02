//! Pure date/time helpers the rest of the scheduler builds on: converting
//! domain `TimeOfDay`/`Weekday` into `chrono` types, and the "rollover day"
//! arithmetic that underpins expiry and weekly-count resets (design spec
//! §4.4) — a day runs rollover -> rollover, not midnight -> midnight.
//!
//! Instants are `DateTime<Utc>`; the rollover is a local wall-clock time, so
//! every helper here that crosses between the two takes the [`Zone`] it is
//! evaluated in. A rollover-day therefore lasts 23 or 25 real hours across a
//! DST change.

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, Utc};

use crate::clock::Zone;
use crate::domain::{TimeOfDay, Weekday};

/// Converts a domain `TimeOfDay` (minute resolution, already range-validated)
/// into a `chrono::NaiveTime`.
pub fn to_naive_time(time: TimeOfDay) -> NaiveTime {
    NaiveTime::from_hms_opt(time.hour() as u32, time.minute() as u32, 0)
        .expect("TimeOfDay already validates hour/minute ranges")
}

/// Converts `chrono`'s weekday enum into the domain's, so at-time recurrences
/// can be matched against a date's day of the week.
pub fn from_chrono_weekday(day: chrono::Weekday) -> Weekday {
    match day {
        chrono::Weekday::Mon => Weekday::Monday,
        chrono::Weekday::Tue => Weekday::Tuesday,
        chrono::Weekday::Wed => Weekday::Wednesday,
        chrono::Weekday::Thu => Weekday::Thursday,
        chrono::Weekday::Fri => Weekday::Friday,
        chrono::Weekday::Sat => Weekday::Saturday,
        chrono::Weekday::Sun => Weekday::Sunday,
    }
}

/// The instant rollover-day `day` begins: the rollover time-of-day on `day`,
/// resolved in `zone`. The day ends where the next one begins.
pub fn rollover_day_start(day: NaiveDate, rollover: TimeOfDay, zone: Zone) -> DateTime<Utc> {
    zone.resolve(day.and_time(to_naive_time(rollover)))
}

/// The "rollover day" an instant belongs to (design spec §4.4): the day runs
/// rollover -> rollover, so an instant before today's rollover time still
/// belongs to yesterday's rollover-day. Compared against the resolved
/// [`rollover_day_start`] rather than the bare wall-clock time, so the days
/// tile the timeline exactly even when the rollover falls in a DST repeat or
/// gap.
pub fn rollover_day(instant: DateTime<Utc>, rollover: TimeOfDay, zone: Zone) -> NaiveDate {
    let date = zone.to_local(instant).date();
    if instant < rollover_day_start(date, rollover, zone) {
        date - Duration::days(1)
    } else {
        date
    }
}

/// The instant a time-of-day `slot` occurs at within rollover-day `day`, in
/// `zone`. A rollover-day spans `[day's rollover, next day's rollover)`, so a
/// slot earlier than the rollover time-of-day falls on the *following*
/// calendar date, not `day` itself — the inverse of [`rollover_day`]. A slot
/// in a fall-back repeat resolves to its first occurrence, and one in a
/// spring-forward gap to the instant the clock jumps to (see
/// [`Zone::resolve`]), so it still occurs exactly once.
pub fn rollover_day_datetime(
    day: NaiveDate,
    slot: NaiveTime,
    rollover: TimeOfDay,
    zone: Zone,
) -> DateTime<Utc> {
    let rollover_time = to_naive_time(rollover);
    let date = if slot >= rollover_time {
        day
    } else {
        day + Duration::days(1)
    };
    zone.resolve(date.and_time(slot))
}

/// The Monday that starts the rollover-defined week containing `day`.
pub fn week_start(day: NaiveDate) -> NaiveDate {
    day - Duration::days(day.weekday().num_days_from_monday() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{london, LONDON};

    fn rollover_04_00() -> TimeOfDay {
        TimeOfDay::new(4, 0).expect("valid time")
    }

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("valid date")
    }

    #[test]
    fn an_instant_after_rollover_belongs_to_the_same_calendar_day() {
        // Given 09:00, after the 04:00 rollover
        let instant = london(2026, 7, 21, 9, 0);

        // When computing its rollover day
        // Then it belongs to its own calendar date
        assert_eq!(
            rollover_day(instant, rollover_04_00(), LONDON),
            date(2026, 7, 21)
        );
    }

    #[test]
    fn an_instant_before_rollover_belongs_to_the_previous_calendar_day() {
        // Given 02:00, before the 04:00 rollover — still "yesterday" for
        // scheduling purposes
        let instant = london(2026, 7, 21, 2, 0);

        // When computing its rollover day
        // Then it belongs to the previous calendar date
        assert_eq!(
            rollover_day(instant, rollover_04_00(), LONDON),
            date(2026, 7, 20)
        );
    }

    #[test]
    fn the_rollover_day_follows_local_time_not_utc() {
        // Given 03:30 UTC on a summer morning — already 04:30 BST, past the
        // 04:00 rollover in London
        let instant = london(2026, 7, 22, 4, 30);
        assert_eq!(instant.format("%H:%M").to_string(), "03:30");

        // When computing its rollover day
        // Then it belongs to the new local day, not the UTC-reading previous one
        assert_eq!(
            rollover_day(instant, rollover_04_00(), LONDON),
            date(2026, 7, 22)
        );
    }

    #[test]
    fn rollover_day_datetime_is_the_inverse_of_rollover_day() {
        // Given a handful of instants either side of a 04:00 rollover
        let rollover = rollover_04_00();
        for (hour, minute) in [(2, 0), (4, 0), (9, 0), (23, 59)] {
            let instant = london(2026, 7, 21, hour, minute);

            // When mapping to a rollover day and back to a real instant
            let day = rollover_day(instant, rollover, LONDON);
            let slot = LONDON.to_local(instant).time();
            let rebuilt = rollover_day_datetime(day, slot, rollover, LONDON);

            // Then the original instant is reproduced exactly
            assert_eq!(rebuilt, instant);
        }
    }

    #[test]
    fn a_slot_before_the_rollover_time_falls_on_the_following_calendar_date() {
        // Given rollover-day 2026-07-20 and a slot of 02:00 (before the 04:00
        // rollover time-of-day)
        let slot = NaiveTime::from_hms_opt(2, 0, 0).expect("valid time");

        // When computing the real instant that slot occurs at
        let instant = rollover_day_datetime(date(2026, 7, 20), slot, rollover_04_00(), LONDON);

        // Then it falls on the next calendar date, not `day` itself
        assert_eq!(instant, london(2026, 7, 21, 2, 0));
    }

    #[test]
    fn both_passes_of_the_fall_back_hour_belong_to_the_previous_rollover_day() {
        // Given 01:30 on 2026-10-25 in London, which happens twice: first in
        // BST (00:30Z), then an hour later in GMT (01:30Z)
        let first_pass = london(2026, 10, 25, 1, 30);
        let second_pass = first_pass + Duration::hours(1);
        assert_eq!(LONDON.to_local(second_pass), LONDON.to_local(first_pass));

        // When computing each one's rollover day under a 04:00 rollover
        // Then both still belong to rollover-day 2026-10-24
        for instant in [first_pass, second_pass] {
            assert_eq!(
                rollover_day(instant, rollover_04_00(), LONDON),
                date(2026, 10, 24)
            );
        }
    }

    #[test]
    fn a_rollover_day_lasts_25_hours_across_the_fall_back_and_23_across_spring_forward() {
        // Given the London rollover-days containing each 2026 DST change
        let rollover = rollover_04_00();
        let length = |day: NaiveDate| {
            rollover_day_start(day + Duration::days(1), rollover, LONDON)
                - rollover_day_start(day, rollover, LONDON)
        };

        // When measuring them from one rollover to the next
        // Then the clock change adds or removes a real hour
        assert_eq!(length(date(2026, 10, 24)), Duration::hours(25));
        assert_eq!(length(date(2026, 3, 28)), Duration::hours(23));
        assert_eq!(length(date(2026, 7, 21)), Duration::hours(24));
    }

    #[test]
    fn a_rollover_inside_the_spring_gap_still_splits_the_days_without_overlap() {
        // Given a 01:30 rollover, a time that never happens on 2026-03-29 in
        // London (the clock jumps 01:00 GMT -> 02:00 BST)
        let rollover = TimeOfDay::new(1, 30).expect("valid time");
        let start = rollover_day_start(date(2026, 3, 29), rollover, LONDON);

        // When classifying the instants either side of that day's start
        // Then the day begins exactly there — read with the pre-jump offset
        assert_eq!(start.format("%H:%M").to_string(), "01:30");
        assert_eq!(
            rollover_day(start - Duration::minutes(1), rollover, LONDON),
            date(2026, 3, 28)
        );
        assert_eq!(rollover_day(start, rollover, LONDON), date(2026, 3, 29));
    }

    #[test]
    fn week_start_returns_the_monday_of_the_containing_week() {
        // Given a Thursday
        let thursday = NaiveDate::from_ymd_opt(2026, 7, 23).expect("valid date");

        // When finding the start of its week
        // Then it is the Monday of that week
        assert_eq!(
            week_start(thursday),
            NaiveDate::from_ymd_opt(2026, 7, 20).expect("valid date")
        );
    }

    #[test]
    fn week_start_of_a_monday_is_itself() {
        // Given a Monday
        let monday = NaiveDate::from_ymd_opt(2026, 7, 20).expect("valid date");

        // When finding the start of its week
        // Then it is unchanged
        assert_eq!(week_start(monday), monday);
    }
}
