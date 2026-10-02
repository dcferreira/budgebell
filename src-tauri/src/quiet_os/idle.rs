//! System idle-time probe (design spec §4.5 "Idle" / §9.2). "Don't nudge an
//! empty chair": if the user hasn't touched keyboard or mouse for a while we
//! treat the chair as empty. On macOS the idle duration comes from IOKit's
//! `HIDIdleTime` (nanoseconds since the last HID event), read fully locally
//! via `ioreg`. On Linux it comes from the session's idle monitor over D-Bus,
//! read via `gdbus`: GNOME's Mutter `GetIdletime` first, then the
//! `org.freedesktop.ScreenSaver` `GetSessionIdleTime` that KDE offers; both
//! report milliseconds. The parsing is pure and unit-tested; the `ioreg` /
//! `gdbus` invocations are the thin impure wrappers.

#[cfg(target_os = "macos")]
use std::process::Command;

use super::error::QuietOsError;
#[cfg(target_os = "linux")]
use super::tool_output::run_stdout_if_available;

/// How long an untouched chair must stay untouched before we call it empty.
/// Short enough to catch a user who has stepped away, long enough not to
/// treat a moment's thought as absence.
pub const IDLE_THRESHOLD_SECS: u64 = 300;

/// Whether an idle duration counts as an empty chair.
pub fn is_idle(idle_secs: u64, threshold_secs: u64) -> bool {
    idle_secs >= threshold_secs
}

/// Extracts the `HIDIdleTime` value (nanoseconds) from `ioreg -c IOHIDSystem`
/// output and converts it to whole seconds. `ioreg` reports every HID device;
/// we take the smallest idle time across them, since a recent event on *any*
/// device means the user is present.
pub fn parse_hid_idle_seconds(ioreg_output: &str) -> Result<u64, QuietOsError> {
    let minimum_nanos = ioreg_output
        .lines()
        .filter_map(parse_hid_idle_line)
        .min()
        .ok_or_else(|| QuietOsError::UnparsableIdle(ioreg_output.to_string()))?;
    Ok(minimum_nanos / 1_000_000_000)
}

/// Parses the nanosecond value from a single `"HIDIdleTime" = 12345` line,
/// returning `None` for any line that isn't a HIDIdleTime entry.
fn parse_hid_idle_line(line: &str) -> Option<u64> {
    let (key, value) = line.split_once('=')?;
    if !key.contains("\"HIDIdleTime\"") {
        return None;
    }
    value.trim().parse().ok()
}

/// Extracts the idle time in milliseconds from `gdbus call` output, which
/// prints the reply as a GVariant tuple: `(uint64 93,)` from Mutter's
/// `GetIdletime`, `(uint32 1234,)` from the ScreenSaver `GetSessionIdleTime`.
pub fn parse_gdbus_idle_millis(gdbus_output: &str) -> Result<u64, QuietOsError> {
    let unparsable = || QuietOsError::UnparsableIdle(gdbus_output.to_string());
    let (kind, value) = gdbus_output
        .trim()
        .strip_prefix('(')
        .and_then(|rest| rest.strip_suffix(",)"))
        .and_then(|inner| inner.split_once(' '))
        .ok_or_else(unparsable)?;
    if kind != "uint64" && kind != "uint32" {
        return Err(unparsable());
    }
    value.trim().parse().map_err(|_| unparsable())
}

/// The D-Bus calls that report idle time, in order of preference: GNOME's
/// Mutter first, then the freedesktop ScreenSaver interface (KDE).
#[cfg(target_os = "linux")]
const GDBUS_IDLE_CALLS: [[&str; 7]; 2] = [
    [
        "call",
        "--session",
        "--dest",
        "org.gnome.Mutter.IdleMonitor",
        "--object-path",
        "/org/gnome/Mutter/IdleMonitor/Core",
        "--method=org.gnome.Mutter.IdleMonitor.GetIdletime",
    ],
    [
        "call",
        "--session",
        "--dest",
        "org.freedesktop.ScreenSaver",
        "--object-path",
        "/org/freedesktop/ScreenSaver",
        "--method=org.freedesktop.ScreenSaver.GetSessionIdleTime",
    ],
];

#[cfg(target_os = "macos")]
pub fn probe_idle() -> Result<bool, QuietOsError> {
    let output = Command::new("ioreg")
        .args(["-c", "IOHIDSystem"])
        .output()
        .map_err(|source| QuietOsError::Spawn {
            probe: "idle",
            source,
        })?;
    if !output.status.success() {
        return Err(QuietOsError::ProbeFailed {
            probe: "idle",
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    let idle_secs = parse_hid_idle_seconds(&String::from_utf8_lossy(&output.stdout))?;
    Ok(is_idle(idle_secs, IDLE_THRESHOLD_SECS))
}

#[cfg(target_os = "linux")]
pub fn probe_idle() -> Result<bool, QuietOsError> {
    // Neither interface being available (no gdbus, no session bus, a desktop
    // that implements neither) degrades to "not idle" rather than erroring: an
    // unconditional error would fail every scheduler tick on such a machine
    // and silently disable reminders altogether. Output from an interface that
    // *did* answer but that we can't parse is a real fault and fails loudly.
    let Some(output) = GDBUS_IDLE_CALLS
        .iter()
        .find_map(|args| run_stdout_if_available("gdbus", args))
    else {
        return Ok(false);
    };
    let idle_secs = parse_gdbus_idle_millis(&output)? / 1000;
    Ok(is_idle(idle_secs, IDLE_THRESHOLD_SECS))
}

// Not implemented on this platform. Degrades to "not idle" — consistent with
// `calendar::list_day_meetings` — rather than erroring, since an
// unconditional error here would fail every scheduler tick forever and
// silently disable reminders altogether.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn probe_idle() -> Result<bool, QuietOsError> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_IOREG: &str = r#"
    +-o IOHIDSystem  <class IOHIDSystem>
        {
          "HIDIdleTime" = 428000000000
          "HIDPointerAcceleration" = 45056
        }
    +-o AppleUSBHostDevice
        {
          "HIDIdleTime" = 12000000000
        }
    "#;

    #[test]
    fn parsing_takes_the_smallest_idle_time_across_devices_and_converts_to_seconds() {
        // Given ioreg output with two HIDIdleTime entries (428s and 12s)
        // When parsed
        let seconds = parse_hid_idle_seconds(SAMPLE_IOREG).expect("parses");

        // Then the most-recent event wins — 12 seconds, not 428
        assert_eq!(seconds, 12);
    }

    #[test]
    fn output_without_any_hid_idle_time_fails_loudly() {
        // Given ioreg output that mentions no HIDIdleTime at all
        // When parsed
        let result = parse_hid_idle_seconds("+-o Root\n  { \"Foo\" = 1 }");

        // Then it errors rather than guessing an idle time
        assert!(matches!(result, Err(QuietOsError::UnparsableIdle(_))));
    }

    #[test]
    fn an_idle_duration_at_or_beyond_the_threshold_is_an_empty_chair() {
        // Given the default five-minute threshold
        // Then exactly-at and beyond both count as idle, just-under does not
        assert!(!is_idle(299, IDLE_THRESHOLD_SECS));
        assert!(is_idle(300, IDLE_THRESHOLD_SECS));
        assert!(is_idle(600, IDLE_THRESHOLD_SECS));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    #[test]
    fn on_unsupported_platforms_the_probe_degrades_to_not_idle_rather_than_erroring() {
        // Given a platform with no idle probe implementation
        // When probed
        // Then it reports not idle instead of failing the scheduler tick
        assert!(matches!(probe_idle(), Ok(false)));
    }

    #[test]
    fn gdbus_parsing_reads_a_mutter_uint64_reply() {
        // Given Mutter's GetIdletime reply
        // When parsed
        let millis = parse_gdbus_idle_millis("(uint64 93,)\n").expect("parses");

        // Then the idle time is the number of milliseconds
        assert_eq!(millis, 93);
    }

    #[test]
    fn gdbus_parsing_reads_a_screensaver_uint32_reply() {
        // Given the ScreenSaver GetSessionIdleTime reply
        // When parsed
        let millis = parse_gdbus_idle_millis("(uint32 1234,)").expect("parses");

        // Then the idle time is the number of milliseconds
        assert_eq!(millis, 1234);
    }

    #[test]
    fn gdbus_parsing_rejects_output_that_is_not_an_idle_tuple() {
        // Given replies that are not a single unsigned integer tuple
        for output in ["", "banana", "(string 'x',)", "(uint64 abc,)", "(uint64 5)"] {
            // When parsed
            let result = parse_gdbus_idle_millis(output);

            // Then it errors rather than guessing an idle time
            assert!(
                matches!(result, Err(QuietOsError::UnparsableIdle(_))),
                "{output:?} should be unparsable"
            );
        }
    }
}
