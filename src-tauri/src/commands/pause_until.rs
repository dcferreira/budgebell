//! Resolving a pause request into a concrete "paused until" instant (design
//! spec §3.3/§3.4): either a duration from now, or an explicit instant.

use chrono::{Duration, NaiveDateTime};

use super::error::CommandError;

/// Resolves exactly one of `duration_secs` or `until` into a "paused until"
/// instant. Rejects anything else: no duration, both, a non-positive
/// duration, or an instant that isn't in the future.
pub fn resolve_pause_until(
    now: NaiveDateTime,
    duration_secs: Option<i64>,
    until: Option<NaiveDateTime>,
) -> Result<NaiveDateTime, CommandError> {
    match (duration_secs, until) {
        (Some(secs), None) if secs > 0 => Ok(now + Duration::seconds(secs)),
        (Some(secs), None) => Err(CommandError::InvalidPauseRequest(format!(
            "duration_secs must be positive, got {secs}"
        ))),
        (None, Some(until)) if until > now => Ok(until),
        (None, Some(_)) => Err(CommandError::InvalidPauseRequest(
            "the pause-until instant must be in the future".to_string(),
        )),
        (None, None) => Err(CommandError::InvalidPauseRequest(
            "either duration_secs or until must be provided".to_string(),
        )),
        (Some(_), Some(_)) => Err(CommandError::InvalidPauseRequest(
            "provide either duration_secs or until, not both".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 7, 21)
            .expect("valid date")
            .and_hms_opt(10, 0, 0)
            .expect("valid time")
    }

    #[test]
    fn a_duration_resolves_to_now_plus_that_many_seconds() {
        // Given a 30-minute pause request
        // When resolving it
        let result = resolve_pause_until(now(), Some(1_800), None).expect("resolves");

        // Then it resolves to now plus 30 minutes
        assert_eq!(result, now() + Duration::seconds(1_800));
    }

    #[test]
    fn a_future_instant_resolves_to_itself() {
        // Given an explicit "resume at" instant in the future
        let until = now() + Duration::hours(1);

        // When resolving it
        let result = resolve_pause_until(now(), None, Some(until)).expect("resolves");

        // Then it resolves to that exact instant
        assert_eq!(result, until);
    }

    #[test]
    fn neither_a_duration_nor_an_instant_is_rejected() {
        // Given no pause parameters at all
        // When resolving it
        let result = resolve_pause_until(now(), None, None);

        // Then it fails loudly rather than guessing a default
        assert!(matches!(result, Err(CommandError::InvalidPauseRequest(_))));
    }

    #[test]
    fn both_a_duration_and_an_instant_is_rejected() {
        // Given both a duration and an explicit instant
        // When resolving it
        let result = resolve_pause_until(now(), Some(60), Some(now() + Duration::hours(1)));

        // Then it fails loudly rather than silently picking one
        assert!(matches!(result, Err(CommandError::InvalidPauseRequest(_))));
    }

    #[test]
    fn a_non_positive_duration_is_rejected() {
        // Given a zero-second duration
        // When resolving it
        let result = resolve_pause_until(now(), Some(0), None);

        // Then it fails loudly rather than pausing for no time at all
        assert!(matches!(result, Err(CommandError::InvalidPauseRequest(_))));
    }

    #[test]
    fn an_instant_in_the_past_is_rejected() {
        // Given a "resume at" instant that has already passed
        let until = now() - Duration::hours(1);

        // When resolving it
        let result = resolve_pause_until(now(), None, Some(until));

        // Then it fails loudly rather than pausing until a moment already gone
        assert!(matches!(result, Err(CommandError::InvalidPauseRequest(_))));
    }
}
