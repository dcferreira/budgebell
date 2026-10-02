//! The local time zone that wall-clock rules (rollover, day window, at-times)
//! are evaluated in. Every instant in the backend is a `DateTime<Utc>`; a
//! [`Zone`] converts between those instants and local wall-clock times at the
//! point a wall-clock rule is applied.

use chrono::{DateTime, Duration, Local, NaiveDateTime, Offset, TimeZone, Utc};

/// The local time zone wall-clock rules are evaluated in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// The OS zone (`chrono::Local`) — production.
    System,
    /// A named IANA zone — tests (DST cases), via the `chrono-tz` dev-dependency.
    #[cfg(test)]
    Named(chrono_tz::Tz),
}

impl Zone {
    /// The wall-clock time in this zone at the given instant.
    pub fn to_local(self, instant: DateTime<Utc>) -> NaiveDateTime {
        match self {
            Zone::System => instant.with_timezone(&Local).naive_local(),
            #[cfg(test)]
            Zone::Named(tz) => instant.with_timezone(&tz).naive_local(),
        }
    }

    /// Wall-clock -> instant. Ambiguous (fall-back repeat hour): the EARLIER
    /// instant. Non-existent (spring-forward gap): the instant the clock jumps
    /// to, i.e. interpret with the pre-transition offset (02:30 in a
    /// 02:00->03:00 gap == 03:30 after the jump).
    pub fn resolve(self, local: NaiveDateTime) -> DateTime<Utc> {
        match self {
            Zone::System => resolve_in(&Local, local),
            #[cfg(test)]
            Zone::Named(tz) => resolve_in(&tz, local),
        }
    }
}

/// Europe/London — the zone the backend's tests evaluate wall-clock rules in,
/// so they are deterministic whatever the host's zone (and can exercise the
/// UK's DST transitions).
#[cfg(test)]
pub const LONDON: Zone = Zone::Named(chrono_tz::Tz::Europe__London);

/// The instant a London wall-clock time occurs at (see [`Zone::resolve`] for
/// the fall-back/spring-forward rules) — lets tests write instants as the
/// local times they read as.
#[cfg(test)]
pub fn london(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
    let local = chrono::NaiveDate::from_ymd_opt(y, mo, d)
        .and_then(|date| date.and_hms_opt(h, mi, 0))
        .expect("valid date and time");
    LONDON.resolve(local)
}

fn resolve_in<Tz: TimeZone>(tz: &Tz, local: NaiveDateTime) -> DateTime<Utc> {
    if let Some(instant) = tz.from_local_datetime(&local).earliest() {
        return instant.with_timezone(&Utc);
    }
    // In a gap: apply the offset in force just before the transition. A day
    // earlier is safely before any real-world gap.
    let before = local - Duration::days(1);
    match tz.from_local_datetime(&before).earliest() {
        Some(earlier) => {
            let offset = earlier.offset().fix();
            (local - Duration::seconds(i64::from(offset.local_minus_utc()))).and_utc()
        }
        // Unresolvable even a day earlier (e.g. out-of-range date): treat the
        // wall-clock as UTC rather than panic.
        None => local.and_utc(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use chrono_tz::Tz;

    fn london() -> Zone {
        Zone::Named(Tz::Europe__London)
    }

    fn vienna() -> Zone {
        Zone::Named(Tz::Europe__Vienna)
    }

    fn naive(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d)
            .and_then(|date| date.and_hms_opt(h, mi, 0))
            .expect("valid date")
    }

    fn utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        naive(y, mo, d, h, mi).and_utc()
    }

    #[test]
    fn to_local_applies_the_zone_offset_in_summer_and_winter() {
        // Given instants in London (BST in summer, GMT in winter) and Vienna (CEST)
        // When converting to local wall-clock time
        // Then the offset in force at that instant is applied
        assert_eq!(
            london().to_local(utc(2026, 7, 1, 12, 0)),
            naive(2026, 7, 1, 13, 0)
        );
        assert_eq!(
            london().to_local(utc(2026, 1, 1, 12, 0)),
            naive(2026, 1, 1, 12, 0)
        );
        assert_eq!(
            vienna().to_local(utc(2026, 7, 1, 12, 0)),
            naive(2026, 7, 1, 14, 0)
        );
    }

    #[test]
    fn resolve_inverts_to_local_for_unambiguous_times() {
        // Given an ordinary instant in each zone
        for zone in [london(), vienna()] {
            let instant = utc(2026, 6, 15, 9, 30);

            // When converting to local and resolving back
            let round_trip = zone.resolve(zone.to_local(instant));

            // Then the original instant is recovered
            assert_eq!(round_trip, instant);
        }
    }

    #[test]
    fn resolve_picks_the_earlier_instant_in_the_fall_back_hour() {
        // Given 01:30 on 2026-10-25 in London, which occurs twice
        // (00:30Z in BST and 01:30Z in GMT)
        let local = naive(2026, 10, 25, 1, 30);

        // When resolving it
        let instant = london().resolve(local);

        // Then the earlier instant is chosen
        assert_eq!(instant, utc(2026, 10, 25, 0, 30));
    }

    #[test]
    fn resolve_interprets_a_spring_forward_gap_with_the_pre_transition_offset() {
        // Given 01:30 on 2026-03-29 in London, which does not exist
        // (the clock jumps from 01:00 GMT to 02:00 BST)
        let local = naive(2026, 3, 29, 1, 30);

        // When resolving it
        let instant = london().resolve(local);

        // Then it is read with the GMT offset, landing at 01:30Z (02:30 BST)
        assert_eq!(instant, utc(2026, 3, 29, 1, 30));
        assert_eq!(london().to_local(instant), naive(2026, 3, 29, 2, 30));
    }

    #[test]
    fn resolve_handles_the_vienna_gap_and_fold() {
        // Given Vienna's 2026-03-29 gap (02:00->03:00) and 2026-10-25 fold (03:00->02:00)
        // When resolving a time inside each
        // Then the gap uses the CET offset and the fold picks the earlier (CEST) instant
        assert_eq!(
            vienna().resolve(naive(2026, 3, 29, 2, 30)),
            utc(2026, 3, 29, 1, 30)
        );
        assert_eq!(
            vienna().resolve(naive(2026, 10, 25, 2, 30)),
            utc(2026, 10, 25, 0, 30)
        );
    }

    #[test]
    fn the_system_zone_round_trips_an_instant() {
        // Given an instant well away from any DST transition
        let instant = utc(2026, 6, 15, 9, 30);

        // When converting through the OS zone and back
        let round_trip = Zone::System.resolve(Zone::System.to_local(instant));

        // Then the instant is unchanged
        assert_eq!(round_trip, instant);
    }
}
