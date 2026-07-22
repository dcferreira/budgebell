//! Pure date/time helpers the rest of the scheduler builds on: converting
//! domain `TimeOfDay`/`Weekday` into `chrono` types, and the "rollover day"
//! arithmetic that underpins expiry and weekly-count resets (design spec
//! §4.4) — a day runs rollover -> rollover, not midnight -> midnight.

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, NaiveTime};

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

/// The "rollover day" an instant belongs to (design spec §4.4): the day runs
/// rollover -> rollover, so an instant before today's rollover time still
/// belongs to yesterday's rollover-day.
pub fn rollover_day(instant: NaiveDateTime, rollover: TimeOfDay) -> NaiveDate {
    if instant.time() < to_naive_time(rollover) {
        instant.date() - Duration::days(1)
    } else {
        instant.date()
    }
}

/// The real calendar instant a time-of-day `slot` occurs at within
/// rollover-day `day`. A rollover-day spans `[day's rollover, next day's
/// rollover)`, so a slot earlier than the rollover time-of-day falls on the
/// *following* calendar date, not `day` itself — the inverse of
/// [`rollover_day`].
pub fn rollover_day_datetime(
    day: NaiveDate,
    slot: NaiveTime,
    rollover: TimeOfDay,
) -> NaiveDateTime {
    let rollover_time = to_naive_time(rollover);
    if slot >= rollover_time {
        day.and_time(slot)
    } else {
        (day + Duration::days(1)).and_time(slot)
    }
}

/// The Monday that starts the rollover-defined week containing `day`.
pub fn week_start(day: NaiveDate) -> NaiveDate {
    day - Duration::days(day.weekday().num_days_from_monday() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rollover_04_00() -> TimeOfDay {
        TimeOfDay::new(4, 0).expect("valid time")
    }

    #[test]
    fn an_instant_after_rollover_belongs_to_the_same_calendar_day() {
        // Given 09:00, after the 04:00 rollover
        let instant = NaiveDate::from_ymd_opt(2026, 7, 21)
            .expect("valid date")
            .and_hms_opt(9, 0, 0)
            .expect("valid time");

        // When computing its rollover day
        // Then it belongs to its own calendar date
        assert_eq!(
            rollover_day(instant, rollover_04_00()),
            NaiveDate::from_ymd_opt(2026, 7, 21).expect("valid date")
        );
    }

    #[test]
    fn an_instant_before_rollover_belongs_to_the_previous_calendar_day() {
        // Given 02:00, before the 04:00 rollover — still "yesterday" for
        // scheduling purposes
        let instant = NaiveDate::from_ymd_opt(2026, 7, 21)
            .expect("valid date")
            .and_hms_opt(2, 0, 0)
            .expect("valid time");

        // When computing its rollover day
        // Then it belongs to the previous calendar date
        assert_eq!(
            rollover_day(instant, rollover_04_00()),
            NaiveDate::from_ymd_opt(2026, 7, 20).expect("valid date")
        );
    }

    #[test]
    fn rollover_day_datetime_is_the_inverse_of_rollover_day() {
        // Given a handful of instants either side of a 04:00 rollover
        let rollover = rollover_04_00();
        for (hour, minute) in [(2, 0), (4, 0), (9, 0), (23, 59)] {
            let instant = NaiveDate::from_ymd_opt(2026, 7, 21)
                .expect("valid date")
                .and_hms_opt(hour, minute, 0)
                .expect("valid time");

            // When mapping to a rollover day and back to a real instant
            let day = rollover_day(instant, rollover);
            let rebuilt = rollover_day_datetime(day, instant.time(), rollover);

            // Then the original instant is reproduced exactly
            assert_eq!(rebuilt, instant);
        }
    }

    #[test]
    fn a_slot_before_the_rollover_time_falls_on_the_following_calendar_date() {
        // Given rollover-day 2026-07-20 and a slot of 02:00 (before the 04:00
        // rollover time-of-day)
        let day = NaiveDate::from_ymd_opt(2026, 7, 20).expect("valid date");
        let slot = NaiveTime::from_hms_opt(2, 0, 0).expect("valid time");

        // When computing the real instant that slot occurs at
        let instant = rollover_day_datetime(day, slot, rollover_04_00());

        // Then it falls on the next calendar date, not `day` itself
        assert_eq!(
            instant,
            NaiveDate::from_ymd_opt(2026, 7, 21)
                .expect("valid date")
                .and_hms_opt(2, 0, 0)
                .expect("valid time")
        );
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
