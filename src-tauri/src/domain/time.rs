use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::error::DomainError;

/// A time of day, minute resolution (design spec §4.2/§4.4 — at-time triggers,
/// day rollover, and the global day window are all expressed this way).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TimeOfDay {
    hour: u8,
    minute: u8,
}

impl TimeOfDay {
    /// Builds a time of day, rejecting out-of-range hours/minutes.
    pub fn new(hour: u8, minute: u8) -> Result<Self, DomainError> {
        if hour > 23 || minute > 59 {
            return Err(DomainError::InvalidTimeOfDay {
                hour: hour as u32,
                minute: minute as u32,
            });
        }
        Ok(Self { hour, minute })
    }

    pub fn hour(self) -> u8 {
        self.hour
    }

    pub fn minute(self) -> u8 {
        self.minute
    }
}

impl FromStr for TimeOfDay {
    type Err = DomainError;

    /// Parses the "HH:MM" format the store uses for `day_rollover`,
    /// `day_window_start`/`_end`, and rotation `window_start`/`_end`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (hour_str, minute_str) = s
            .split_once(':')
            .ok_or_else(|| DomainError::UnparsableTimeOfDay(s.to_string()))?;
        let hour: u8 = hour_str
            .parse()
            .map_err(|_| DomainError::UnparsableTimeOfDay(s.to_string()))?;
        let minute: u8 = minute_str
            .parse()
            .map_err(|_| DomainError::UnparsableTimeOfDay(s.to_string()))?;
        Self::new(hour, minute)
    }
}

impl fmt::Display for TimeOfDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

/// A start–end window of times of day (design spec §4.3/§4.4 — a rotation's
/// own window, or the global day window).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeWindow {
    pub start: TimeOfDay,
    pub end: TimeOfDay,
}

impl TimeWindow {
    /// Builds a window, rejecting a degenerate start == end (which would
    /// never bound anything). Overnight windows (start > end) are allowed —
    /// the scheduler is responsible for interpreting the wrap-around.
    pub fn new(start: TimeOfDay, end: TimeOfDay) -> Result<Self, DomainError> {
        if start == end {
            return Err(DomainError::DegenerateTimeWindow);
        }
        Ok(Self { start, end })
    }
}

/// A day of the week, used by the `specific-weekdays` recurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Weekday {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing_a_valid_hh_mm_string_yields_the_matching_time_of_day() {
        // Given a well-formed "HH:MM" string
        // When it is parsed
        let time: TimeOfDay = "09:05".parse().expect("parses");

        // Then the hour and minute match
        assert_eq!(time.hour(), 9);
        assert_eq!(time.minute(), 5);
    }

    #[test]
    fn displaying_a_time_of_day_zero_pads_back_to_hh_mm() {
        // Given a time of day built from single-digit components
        let time = TimeOfDay::new(4, 0).expect("valid time");

        // When displayed
        // Then it round-trips to the zero-padded "HH:MM" form the store uses
        assert_eq!(time.to_string(), "04:00");
    }

    #[test]
    fn parsing_and_displaying_round_trips_for_every_default_config_value() {
        // Given the design spec's default config strings (§7)
        for raw in ["04:00", "09:00", "18:00"] {
            // When parsed then displayed again
            let time: TimeOfDay = raw.parse().expect("parses");

            // Then the original string is reproduced exactly
            assert_eq!(time.to_string(), raw);
        }
    }

    #[test]
    fn an_hour_above_23_is_rejected() {
        // Given an hour past the valid range
        // When building a TimeOfDay
        let result = TimeOfDay::new(24, 0);

        // Then it fails loudly rather than wrapping or clamping
        assert!(matches!(
            result,
            Err(DomainError::InvalidTimeOfDay {
                hour: 24,
                minute: 0
            })
        ));
    }

    #[test]
    fn a_minute_above_59_is_rejected() {
        // Given a minute past the valid range
        // When building a TimeOfDay
        let result = TimeOfDay::new(9, 60);

        // Then it fails loudly rather than wrapping or clamping
        assert!(matches!(
            result,
            Err(DomainError::InvalidTimeOfDay {
                hour: 9,
                minute: 60
            })
        ));
    }

    #[test]
    fn a_string_without_a_colon_is_unparsable() {
        // Given a malformed time string
        // When parsing it
        let result: Result<TimeOfDay, _> = "0900".parse();

        // Then it fails loudly rather than guessing
        assert!(matches!(result, Err(DomainError::UnparsableTimeOfDay(_))));
    }

    #[test]
    fn a_window_with_equal_start_and_end_is_degenerate_and_rejected() {
        // Given a start and end that are identical
        let same = TimeOfDay::new(9, 0).expect("valid time");

        // When building a TimeWindow from them
        let result = TimeWindow::new(same, same);

        // Then it is rejected as degenerate
        assert!(matches!(result, Err(DomainError::DegenerateTimeWindow)));
    }

    #[test]
    fn an_overnight_window_where_start_is_after_end_is_allowed() {
        // Given a window that wraps past midnight
        let start = TimeOfDay::new(22, 0).expect("valid time");
        let end = TimeOfDay::new(6, 0).expect("valid time");

        // When building a TimeWindow from them
        let window = TimeWindow::new(start, end).expect("overnight windows are valid");

        // Then it is accepted — interpreting the wrap-around is the scheduler's job
        assert_eq!(window.start, start);
        assert_eq!(window.end, end);
    }

    #[test]
    fn a_weekday_round_trips_through_json_as_kebab_case() {
        // Given a weekday
        // When serialised to JSON
        let json = serde_json::to_string(&Weekday::Wednesday).expect("serialises");

        // Then it uses the kebab-case rendering
        assert_eq!(json, "\"wednesday\"");

        // And deserialises back to the same value
        let round_tripped: Weekday = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(round_tripped, Weekday::Wednesday);
    }
}
