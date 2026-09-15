//! Microphone-in-use probe (design spec §4.5). "Don't nudge during a call":
//! an active default input device is a strong proxy for an ongoing call —
//! including the ad-hoc, off-calendar ones the calendar probe cannot see. A
//! tiny ad-hoc-signed Swift/CoreAudio helper (`helpers/mic_probe.swift`,
//! compiled by `build.rs`) queries CoreAudio's
//! `kAudioDevicePropertyDeviceIsRunningSomewhere` on the default input device
//! and prints "1" (capturing) or "0" (idle). That is a device-property query,
//! NOT audio capture, so it neither requires nor triggers the microphone TCC
//! permission prompt. Fully local — no network I/O. The classifier here is
//! pure and unit-tested; only `probe_mic_in_use` touches the OS.

#[cfg(target_os = "macos")]
use std::process::Command;

use super::error::QuietOsError;

/// Interprets the helper's single-line output: "1" means the default input
/// device is capturing somewhere (mic in use), "0" means it is idle. Anything
/// else fails loudly rather than assuming the user is free.
pub fn parse_mic_running(output: &str) -> Result<bool, QuietOsError> {
    match output.trim() {
        "1" => Ok(true),
        "0" => Ok(false),
        other => Err(QuietOsError::UnparsableMic(other.to_string())),
    }
}

#[cfg(target_os = "macos")]
pub fn probe_mic_in_use() -> Result<bool, QuietOsError> {
    let output = Command::new(env!("MIC_PROBE_PATH"))
        .output()
        .map_err(|source| QuietOsError::Spawn {
            probe: "mic",
            source,
        })?;
    if !output.status.success() {
        return Err(QuietOsError::ProbeFailed {
            probe: "mic",
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    parse_mic_running(&String::from_utf8_lossy(&output.stdout))
}

// Not yet implemented on this platform. Degrades to "mic not in use" —
// consistent with `calendar::list_day_meetings` — rather than erroring, since
// an unconditional error here would fail every scheduler tick forever and
// silently disable reminders altogether.
#[cfg(not(target_os = "macos"))]
pub fn probe_mic_in_use() -> Result<bool, QuietOsError> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_running_input_device_means_the_mic_is_in_use() {
        // Given the helper reports the default input device is capturing
        // When the output is classified
        // Then the mic is in use
        assert!(parse_mic_running("1").expect("parses"));
    }

    #[test]
    fn an_idle_input_device_means_the_mic_is_not_in_use() {
        // Given the helper reports the default input device is idle
        // When the output is classified
        // Then the mic is not in use
        assert!(!parse_mic_running("0").expect("parses"));
    }

    #[test]
    fn surrounding_whitespace_and_a_trailing_newline_are_tolerated() {
        // Given the helper's line-buffered output carries a trailing newline
        // When the output is classified
        // Then the value is read regardless of the surrounding whitespace
        assert!(parse_mic_running("1\n").expect("parses"));
        assert!(!parse_mic_running("  0  \n").expect("parses"));
    }

    #[test]
    fn unrecognised_output_fails_loudly_rather_than_assuming_the_user_is_free() {
        // Given output that is neither "1" nor "0"
        // When the output is classified
        let result = parse_mic_running("banana");

        // Then it errors instead of silently returning "not in use"
        assert!(matches!(result, Err(QuietOsError::UnparsableMic(_))));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn on_non_macos_the_probe_degrades_to_not_in_use_rather_than_erroring() {
        // Given a platform with no microphone probe implementation
        // When probed
        // Then it reports the mic as not in use instead of failing the scheduler tick
        assert!(matches!(probe_mic_in_use(), Ok(false)));
    }
}
