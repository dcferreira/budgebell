//! Microphone-in-use probe (design spec §4.5). "Don't nudge during a call":
//! an active default input device is a strong proxy for an ongoing call —
//! including the ad-hoc, off-calendar ones the calendar probe cannot see. A
//! tiny ad-hoc-signed Swift/CoreAudio helper (`helpers/mic_probe.swift`,
//! compiled by `build.rs` and bundled as a sidecar) queries CoreAudio's
//! `kAudioDevicePropertyDeviceIsRunningSomewhere` on the default input device
//! and prints "1" (capturing) or "0" (idle). That is a device-property query,
//! NOT audio capture, so it neither requires nor triggers the microphone TCC
//! permission prompt. On Linux the sound server is asked instead:
//! `pactl -f json list sources` reports each capture source's state, and any
//! non-monitor source that is `RUNNING` means an app is recording. Fully
//! local — no network I/O. The classifiers here are pure and unit-tested; only
//! `probe_mic_in_use` touches the OS.

#[cfg(target_os = "macos")]
use std::process::Command;

use serde_json::Value;

use super::error::QuietOsError;
#[cfg(target_os = "macos")]
use super::probe_path;
#[cfg(target_os = "linux")]
use super::tool_output::run_stdout_if_available;

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

/// Whether any real capture source is in use, given the output of
/// `pactl -f json list sources`. A source is capturing when its `state` is
/// `RUNNING` and it is not a sink monitor (`device.class` of `monitor`, or a
/// name ending in `.monitor`) — a running monitor only means audio is playing,
/// not that a microphone is open. Virtual sources with no `device.class`, such
/// as EasyEffects' `easyeffects_source`, do count.
pub fn parse_pactl_sources_capturing(pactl_json: &str) -> Result<bool, QuietOsError> {
    let sources: Value = serde_json::from_str(pactl_json).map_err(|source| QuietOsError::Json {
        probe: "mic",
        source,
    })?;
    let capturing = sources
        .as_array()
        .into_iter()
        .flatten()
        .any(|source| is_running(source) && !is_monitor(source));
    Ok(capturing)
}

fn is_running(source: &Value) -> bool {
    source.get("state").and_then(Value::as_str) == Some("RUNNING")
}

fn is_monitor(source: &Value) -> bool {
    let device_class = source
        .get("properties")
        .and_then(|properties| properties.get("device.class"))
        .and_then(Value::as_str);
    let name = source.get("name").and_then(Value::as_str).unwrap_or("");
    device_class == Some("monitor") || name.ends_with(".monitor")
}

#[cfg(target_os = "macos")]
pub fn probe_mic_in_use() -> Result<bool, QuietOsError> {
    let probe = probe_path::resolve("mic_probe").map_err(|source| QuietOsError::Spawn {
        probe: "mic",
        source,
    })?;
    let output = Command::new(probe)
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

#[cfg(target_os = "linux")]
pub fn probe_mic_in_use() -> Result<bool, QuietOsError> {
    // A missing `pactl` or no reachable sound server exits nonzero or fails to
    // spawn; that degrades to "mic not in use" rather than erroring, since an
    // unconditional error would fail every scheduler tick on such a machine
    // and silently disable reminders altogether. JSON we can't parse from a
    // `pactl` that *did* answer fails loudly.
    match run_stdout_if_available("pactl", &["-f", "json", "list", "sources"]) {
        Some(output) => parse_pactl_sources_capturing(&output),
        None => Ok(false),
    }
}

// Not implemented on this platform. Degrades to "mic not in use" —
// consistent with `calendar::list_day_meetings` — rather than erroring, since
// an unconditional error here would fail every scheduler tick forever and
// silently disable reminders altogether.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
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

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    #[test]
    fn on_unsupported_platforms_the_probe_degrades_to_not_in_use_rather_than_erroring() {
        // Given a platform with no microphone probe implementation
        // When probed
        // Then it reports the mic as not in use instead of failing the scheduler tick
        assert!(matches!(probe_mic_in_use(), Ok(false)));
    }

    fn sources_json(sources: &[(&str, &str, Option<&str>)]) -> String {
        let entries: Vec<String> = sources
            .iter()
            .map(|(name, state, class)| {
                let properties = match class {
                    Some(class) => format!(r#"{{ "device.class": "{class}" }}"#),
                    None => "{}".to_string(),
                };
                format!(r#"{{ "name": "{name}", "state": "{state}", "properties": {properties} }}"#)
            })
            .collect();
        format!("[{}]", entries.join(","))
    }

    #[test]
    fn pactl_sources_that_are_all_suspended_mean_the_mic_is_not_in_use() {
        // Given a mic and a sink monitor that are both suspended
        let json = sources_json(&[
            ("alsa_input.usb-mic", "SUSPENDED", Some("sound")),
            ("alsa_output.pci.monitor", "SUSPENDED", Some("monitor")),
        ]);

        // When classified
        // Then nothing is capturing
        assert!(!parse_pactl_sources_capturing(&json).expect("parses"));
    }

    #[test]
    fn a_running_hardware_mic_means_the_mic_is_in_use() {
        // Given a hardware mic that is RUNNING
        let json = sources_json(&[("alsa_input.usb-mic", "RUNNING", Some("sound"))]);

        // When classified
        // Then the mic is in use
        assert!(parse_pactl_sources_capturing(&json).expect("parses"));
    }

    #[test]
    fn a_running_sink_monitor_alone_is_playback_not_a_call() {
        // Given only a sink monitor RUNNING (audio playing, nobody recording)
        let json = sources_json(&[
            ("alsa_output.pci.monitor", "RUNNING", Some("monitor")),
            // A monitor is recognised by its name even without a device class
            ("easyeffects_sink.monitor", "RUNNING", None),
        ]);

        // When classified
        // Then the mic is not in use
        assert!(!parse_pactl_sources_capturing(&json).expect("parses"));
    }

    #[test]
    fn a_running_virtual_source_without_a_device_class_counts_as_capturing() {
        // Given a RUNNING virtual source (easyeffects) with no device.class
        let json = sources_json(&[("easyeffects_source", "RUNNING", None)]);

        // When classified
        // Then the mic is in use, since an app is reading from it
        assert!(parse_pactl_sources_capturing(&json).expect("parses"));
    }

    #[test]
    fn an_empty_source_list_means_the_mic_is_not_in_use() {
        // Given pactl lists no sources at all
        // When classified
        // Then the mic is not in use
        assert!(!parse_pactl_sources_capturing("[]").expect("parses"));
    }

    #[test]
    fn malformed_pactl_json_fails_loudly_rather_than_assuming_the_user_is_free() {
        // Given output that is not valid JSON
        let result = parse_pactl_sources_capturing("not json");

        // Then it errors instead of silently returning "not in use"
        assert!(matches!(
            result,
            Err(QuietOsError::Json { probe: "mic", .. })
        ));
    }
}
