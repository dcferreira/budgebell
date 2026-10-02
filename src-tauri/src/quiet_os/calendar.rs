//! Meeting-aware pause probe (design spec §8). A tiny ad-hoc-signed Swift
//! helper (`helpers/meeting_probe.swift`, compiled by `build.rs`) reads the
//! LOCAL Calendar store via EventKit and emits the current window's events as
//! JSON. This module parses that JSON and decides whether a "real meeting" is
//! happening *now*, honouring the configured mode. Everything here — parsing
//! and classification — is pure and unit-tested; only `probe_real_meeting_now`
//! touches the OS. No network I/O anywhere.

use chrono::{DateTime, NaiveDateTime, Utc};
use serde::Deserialize;

use crate::clock::Zone;
use crate::store::CalendarMode;

use super::error::QuietOsError;
#[cfg(target_os = "macos")]
use super::probe_path;

/// The timestamp format the Swift helper emits and accepts: local wall-clock
/// time with no zone. It is resolved to (or from) a UTC instant in the local
/// [`Zone`] right at this boundary, so nothing past it sees a zone-less time.
const EVENT_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S";

/// One calendar event as reported by the helper, already parsed into UTC
/// instants and an "other attendee" count (attendees who are not the user).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEvent {
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub other_attendee_count: u32,
}

/// The raw JSON shape the Swift helper writes, before timestamp parsing.
#[derive(Debug, Deserialize)]
struct RawEvent {
    #[serde(default)]
    title: String,
    start: String,
    end: String,
    #[serde(rename = "otherAttendeeCount")]
    other_attendee_count: u32,
}

impl RawEvent {
    fn into_event(self, zone: Zone) -> Result<CalendarEvent, QuietOsError> {
        Ok(CalendarEvent {
            title: self.title,
            start: parse_event_time(&self.start, zone)?,
            end: parse_event_time(&self.end, zone)?,
            other_attendee_count: self.other_attendee_count,
        })
    }
}

/// Parses one of the helper's local wall-clock timestamps and resolves it to
/// the instant it denotes in `zone`.
fn parse_event_time(value: &str, zone: Zone) -> Result<DateTime<Utc>, QuietOsError> {
    let local = NaiveDateTime::parse_from_str(value, EVENT_TIME_FORMAT).map_err(|source| {
        QuietOsError::UnparsableEventTime {
            value: value.to_string(),
            source,
        }
    })?;
    Ok(zone.resolve(local))
}

/// Parses the helper's JSON array into typed events, reading its local
/// timestamps in `zone`.
pub fn parse_events(json: &str, zone: Zone) -> Result<Vec<CalendarEvent>, QuietOsError> {
    let raw: Vec<RawEvent> = serde_json::from_str(json).map_err(|source| QuietOsError::Json {
        probe: "calendar",
        source,
    })?;
    raw.into_iter()
        .map(|event| event.into_event(zone))
        .collect()
}

/// The day's events to *display* in the Stats window, filtered to mirror
/// exactly what the meeting-pause rule considers (design spec §3.9): when the
/// calendar isn't being read at all (`enabled == false`) nothing is shown;
/// in `WithOthers` mode only events with at least one other attendee (the
/// "calls") are shown; in `All` mode every event is shown. This is pure so
/// the display set can never drift from the pause rule's own classification.
pub fn meetings_for_day(
    events: &[CalendarEvent],
    mode: CalendarMode,
    enabled: bool,
) -> Vec<CalendarEvent> {
    if !enabled {
        return Vec::new();
    }
    events
        .iter()
        .filter(|event| match mode {
            CalendarMode::All => true,
            CalendarMode::WithOthers => event.other_attendee_count >= 1,
        })
        .cloned()
        .collect()
}

/// Lists the calendar events to display for the rollover-day bounded by
/// `[day_start, day_end)`, already filtered per the calendar config. The
/// helper is handed the bounds as wall-clock times in `zone`. Returns empty
/// without touching the OS when calendar pausing is off, so a disabled
/// calendar is never read (no TCC prompt) — mirroring `probe_quiet_state`'s
/// "a disabled source is not probed" rule.
#[cfg(target_os = "macos")]
pub fn list_day_meetings(
    day_start: DateTime<Utc>,
    day_end: DateTime<Utc>,
    zone: Zone,
    mode: CalendarMode,
    enabled: bool,
) -> Result<Vec<CalendarEvent>, QuietOsError> {
    if !enabled {
        return Ok(Vec::new());
    }
    let events = probe_day_events(day_start, day_end, zone)?;
    Ok(meetings_for_day(&events, mode, true))
}

/// Off macOS there is no local Calendar store to read, so the Stats window
/// simply shows no meetings rather than failing — the display feature degrades
/// gracefully where the meeting-pause rule itself is unsupported.
#[cfg(not(target_os = "macos"))]
pub fn list_day_meetings(
    _day_start: DateTime<Utc>,
    _day_end: DateTime<Utc>,
    _zone: Zone,
    _mode: CalendarMode,
    _enabled: bool,
) -> Result<Vec<CalendarEvent>, QuietOsError> {
    Ok(Vec::new())
}

/// Invokes the helper for an explicit day range and parses its events. The
/// bounds are passed as the same `EVENT_TIME_FORMAT` local wall-clock strings
/// the helper emits — converted into `zone` here — so the helper resolves
/// them in the machine's local zone.
#[cfg(target_os = "macos")]
fn probe_day_events(
    day_start: DateTime<Utc>,
    day_end: DateTime<Utc>,
    zone: Zone,
) -> Result<Vec<CalendarEvent>, QuietOsError> {
    use std::process::Command;

    let probe = probe_path::resolve("meeting_probe").map_err(|source| QuietOsError::Spawn {
        probe: "calendar",
        source,
    })?;
    let output = Command::new(probe)
        .arg(
            zone.to_local(day_start)
                .format(EVENT_TIME_FORMAT)
                .to_string(),
        )
        .arg(zone.to_local(day_end).format(EVENT_TIME_FORMAT).to_string())
        .output()
        .map_err(|source| QuietOsError::Spawn {
            probe: "calendar",
            source,
        })?;
    if !output.status.success() {
        return Err(QuietOsError::ProbeFailed {
            probe: "calendar",
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    parse_events(&String::from_utf8_lossy(&output.stdout), zone)
}

/// Whether a "real meeting" is happening at `now`, per the configured mode
/// (design spec §4.5/§8). An event is happening now when `start <= now < end`.
/// In `All` mode any current event counts; in `WithOthers` mode only an event
/// with at least one other attendee counts (a solo event is a focus block).
pub fn classify_real_meeting_now(
    events: &[CalendarEvent],
    now: DateTime<Utc>,
    mode: CalendarMode,
) -> bool {
    events
        .iter()
        .filter(|event| event.start <= now && now < event.end)
        .any(|event| match mode {
            CalendarMode::All => true,
            CalendarMode::WithOthers => event.other_attendee_count >= 1,
        })
}

#[cfg(target_os = "macos")]
pub fn probe_real_meeting_now(
    now: DateTime<Utc>,
    zone: Zone,
    mode: CalendarMode,
) -> Result<bool, QuietOsError> {
    use std::process::Command;

    let probe = probe_path::resolve("meeting_probe").map_err(|source| QuietOsError::Spawn {
        probe: "calendar",
        source,
    })?;
    let output = Command::new(probe)
        .output()
        .map_err(|source| QuietOsError::Spawn {
            probe: "calendar",
            source,
        })?;
    if !output.status.success() {
        return Err(QuietOsError::ProbeFailed {
            probe: "calendar",
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    let events = parse_events(&String::from_utf8_lossy(&output.stdout), zone)?;
    Ok(classify_real_meeting_now(&events, now, mode))
}

// Not yet implemented on this platform. Degrades to "no meeting" —
// consistent with `list_day_meetings` above — rather than erroring, since an
// unconditional error here would fail every scheduler tick forever and
// silently disable reminders altogether.
#[cfg(not(target_os = "macos"))]
pub fn probe_real_meeting_now(
    _now: DateTime<Utc>,
    _zone: Zone,
    _mode: CalendarMode,
) -> Result<bool, QuietOsError> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{london, LONDON};

    fn dt(hour: u32, minute: u32) -> DateTime<Utc> {
        london(2026, 7, 22, hour, minute)
    }

    fn event(start: DateTime<Utc>, end: DateTime<Utc>, other_attendee_count: u32) -> CalendarEvent {
        CalendarEvent {
            title: "Some event".to_string(),
            start,
            end,
            other_attendee_count,
        }
    }

    #[test]
    fn parses_the_helper_json_into_typed_events_resolved_from_local_time() {
        // Given the JSON the Swift helper emits for one 10:15–11:00 meeting,
        // in local wall-clock time with no zone
        let json = r#"[
            { "title": "Standup", "start": "2026-07-22T10:15:00", "end": "2026-07-22T11:00:00", "otherAttendeeCount": 3 }
        ]"#;

        // When parsed in London (BST, UTC+1)
        let events = parse_events(json, LONDON).expect("parses");

        // Then the single event round-trips into typed fields, its times
        // resolved to the UTC instants they denote (09:15Z–10:00Z)
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "Standup");
        assert_eq!(events[0].start, dt(10, 15));
        assert_eq!(events[0].end, dt(11, 0));
        assert_eq!(events[0].start.format("%H:%M").to_string(), "09:15");
        assert_eq!(events[0].other_attendee_count, 3);
    }

    #[test]
    fn an_unparsable_event_timestamp_fails_loudly() {
        // Given helper JSON with a malformed start time
        let json = r#"[ { "title": "X", "start": "nope", "end": "2026-07-22T11:00:00", "otherAttendeeCount": 1 } ]"#;

        // When parsed
        let result = parse_events(json, LONDON);

        // Then it errors rather than dropping or guessing the event
        assert!(matches!(
            result,
            Err(QuietOsError::UnparsableEventTime { .. })
        ));
    }

    #[test]
    fn example_a_a_meeting_with_others_running_now_is_a_real_meeting_in_with_others_mode() {
        // Given a 10:15–11:00 meeting with three others (design spec §4.7 example A)
        let events = [event(dt(10, 15), dt(11, 0), 3)];

        // When it is 10:30, inside the meeting, in the default with-others mode
        // Then a real meeting is happening now
        assert!(classify_real_meeting_now(
            &events,
            dt(10, 30),
            CalendarMode::WithOthers
        ));
    }

    #[test]
    fn a_solo_event_is_a_focus_block_not_a_real_meeting_in_with_others_mode() {
        // Given a solo 10:00–11:00 event with no other attendees
        let events = [event(dt(10, 0), dt(11, 0), 0)];

        // When it is 10:30 in with-others mode
        // Then it does not count as a real meeting (it is a focus block)
        assert!(!classify_real_meeting_now(
            &events,
            dt(10, 30),
            CalendarMode::WithOthers
        ));
    }

    #[test]
    fn a_solo_event_does_count_as_a_real_meeting_in_all_mode() {
        // Given the same solo 10:00–11:00 event
        let events = [event(dt(10, 0), dt(11, 0), 0)];

        // When it is 10:30 but the mode is "all events"
        // Then any current event counts, solo or not
        assert!(classify_real_meeting_now(
            &events,
            dt(10, 30),
            CalendarMode::All
        ));
    }

    #[test]
    fn an_event_that_has_not_started_or_has_already_ended_is_not_now() {
        // Given a meeting-with-others running 10:15–11:00
        let events = [event(dt(10, 15), dt(11, 0), 2)];

        // Then before it starts and after it ends, no real meeting is now
        assert!(!classify_real_meeting_now(
            &events,
            dt(10, 0),
            CalendarMode::WithOthers
        ));
        assert!(!classify_real_meeting_now(
            &events,
            dt(11, 0),
            CalendarMode::WithOthers
        ));
    }

    #[test]
    fn day_listing_shows_nothing_when_the_calendar_is_not_being_read() {
        // Given a day with a real call, but calendar pausing switched off
        let events = [event(dt(10, 0), dt(10, 30), 2)];

        // When listing the day's meetings with `enabled == false`
        let shown = meetings_for_day(&events, CalendarMode::WithOthers, false);

        // Then nothing is shown — the app isn't reading the calendar at all
        assert!(shown.is_empty());
    }

    #[test]
    fn with_others_mode_lists_only_events_that_have_another_attendee() {
        // Given a solo focus block and a two-person call on the same day
        let solo = event(dt(9, 0), dt(9, 30), 0);
        let call = event(dt(10, 0), dt(10, 30), 1);
        let events = [solo, call.clone()];

        // When listing in the default with-others mode
        let shown = meetings_for_day(&events, CalendarMode::WithOthers, true);

        // Then only the call is shown — the solo block is not a meeting
        assert_eq!(shown, vec![call]);
    }

    #[test]
    fn all_mode_lists_every_event_including_solo_blocks() {
        // Given a solo focus block and a two-person call on the same day
        let solo = event(dt(9, 0), dt(9, 30), 0);
        let call = event(dt(10, 0), dt(10, 30), 1);
        let events = [solo.clone(), call.clone()];

        // When listing in "all events" mode
        let shown = meetings_for_day(&events, CalendarMode::All, true);

        // Then both are shown, in order — every calendar event counts
        assert_eq!(shown, vec![solo, call]);
    }

    #[test]
    fn the_end_instant_is_exclusive_and_the_start_instant_is_inclusive() {
        // Given a meeting running exactly 10:00–10:30 with one other attendee
        let events = [event(dt(10, 0), dt(10, 30), 1)];

        // Then it is "now" at its start but not at its end
        assert!(classify_real_meeting_now(
            &events,
            dt(10, 0),
            CalendarMode::WithOthers
        ));
        assert!(!classify_real_meeting_now(
            &events,
            dt(10, 30),
            CalendarMode::WithOthers
        ));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn on_non_macos_the_probe_degrades_to_no_meeting_rather_than_erroring() {
        // Given a platform with no real-time meeting probe implementation
        // When probed
        // Then it reports no meeting instead of failing the scheduler tick
        assert!(matches!(
            probe_real_meeting_now(dt(10, 0), LONDON, CalendarMode::WithOthers),
            Ok(false)
        ));
    }
}
