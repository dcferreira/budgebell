//! System idle-time probe (design spec §4.5 "Idle" / §9.2). "Don't nudge an
//! empty chair": if the user hasn't touched keyboard or mouse for a while we
//! treat the chair as empty. The idle duration comes from IOKit's
//! `HIDIdleTime` (nanoseconds since the last HID event), read fully locally
//! via `ioreg`. The parsing is pure and unit-tested; the `ioreg` invocation is
//! the thin impure wrapper.

use std::process::Command;

use super::error::QuietOsError;

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

#[cfg(not(target_os = "macos"))]
pub fn probe_idle() -> Result<bool, QuietOsError> {
    Err(QuietOsError::Unsupported)
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
}
