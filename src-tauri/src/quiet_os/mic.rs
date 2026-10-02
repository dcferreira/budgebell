//! Microphone-in-use probe (design spec §4.5). "Don't nudge during a call":
//! an active default input device is a strong proxy for an ongoing call —
//! including the ad-hoc, off-calendar ones the calendar probe cannot see. A
//! tiny ad-hoc-signed Swift/CoreAudio helper (`helpers/mic_probe.swift`,
//! compiled by `build.rs` and bundled as a sidecar) queries CoreAudio's
//! `kAudioDevicePropertyDeviceIsRunningSomewhere` on the default input device
//! and prints "1" (capturing) or "0" (idle). That is a device-property query,
//! NOT audio capture, so it neither requires nor triggers the microphone TCC
//! permission prompt. On Linux the sound server is asked instead: `pactl -f
//! json list source-outputs` lists the recording streams, and a stream that is
//! not corked, not reading a sink monitor (found via `list sources`) and not a
//! helper (pavucontrol / EasyEffects / peak-detect level meters) means an app
//! is recording. Source state is not used, since always-on readers keep a
//! microphone `RUNNING` with nobody on a call. Fully
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

/// Applications whose capture streams are helpers, not a person on a call:
/// pavucontrol's level meters and EasyEffects' own input processing.
const IGNORED_APPLICATION_IDS: [&str; 2] =
    ["org.PulseAudio.pavucontrol", "com.github.wwmm.easyeffects"];

/// Whether any real recording is in progress, given the output of
/// `pactl -f json list sources` and `pactl -f json list source-outputs`.
///
/// Source *state* is deliberately not used: always-on readers (EasyEffects
/// input processing, filter chains, level meters) keep a microphone `RUNNING`
/// with nobody recording. Instead a call is in progress when some
/// source-output (a recording stream) is not `corked`, does not read a sink
/// monitor (`device.class` of `monitor`, or a name ending in `.monitor` —
/// desktop-audio capture is not a microphone), and is not a helper stream:
/// an `application.id` of pavucontrol or EasyEffects, a `media.name` of
/// "Peak detect", or `stream.monitor` of "true" (PipeWire's mark for
/// level-meter peak streams). Virtual sources such as `easyeffects_source`
/// count, so another app recording through EasyEffects is detected.
///
/// Either input that is not a JSON array fails loudly.
pub fn parse_pactl_capturing(
    sources_json: &str,
    source_outputs_json: &str,
) -> Result<bool, QuietOsError> {
    let monitor_indexes: Vec<u64> = parse_json_array(sources_json)?
        .iter()
        .filter(|source| is_monitor(source))
        .filter_map(|source| source.get("index").and_then(Value::as_u64))
        .collect();
    let capturing = parse_json_array(source_outputs_json)?.iter().any(|output| {
        let reads_monitor = output
            .get("source")
            .and_then(Value::as_u64)
            .is_some_and(|index| monitor_indexes.contains(&index));
        !is_corked(output) && !reads_monitor && !is_helper_stream(output)
    });
    Ok(capturing)
}

/// Parses pactl JSON that must be a top-level array.
fn parse_json_array(pactl_json: &str) -> Result<Vec<Value>, QuietOsError> {
    let parsed: Value = serde_json::from_str(pactl_json).map_err(|source| QuietOsError::Json {
        probe: "mic",
        source,
    })?;
    match parsed {
        Value::Array(items) => Ok(items),
        _ => Err(QuietOsError::UnparsableMic(pactl_json.to_string())),
    }
}

fn is_corked(output: &Value) -> bool {
    output.get("corked").and_then(Value::as_bool) == Some(true)
}

fn property<'a>(object: &'a Value, key: &str) -> Option<&'a str> {
    object
        .get("properties")
        .and_then(|properties| properties.get(key))
        .and_then(Value::as_str)
}

fn is_helper_stream(output: &Value) -> bool {
    property(output, "application.id").is_some_and(|id| IGNORED_APPLICATION_IDS.contains(&id))
        || property(output, "media.name") == Some("Peak detect")
        || property(output, "stream.monitor") == Some("true")
}

fn is_monitor(source: &Value) -> bool {
    let name = source.get("name").and_then(Value::as_str).unwrap_or("");
    property(source, "device.class") == Some("monitor") || name.ends_with(".monitor")
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
    probe_mic_in_use_with(run_stdout_if_available)
}

/// The Linux mic probe with the tool runner injected, so the degrade and
/// error handling can be tested without a sound server.
#[cfg(target_os = "linux")]
fn probe_mic_in_use_with(
    run: impl Fn(&str, &[&str]) -> Option<String>,
) -> Result<bool, QuietOsError> {
    // A missing `pactl` or no reachable sound server exits nonzero or fails to
    // spawn; that degrades to "mic not in use" rather than erroring, since an
    // unconditional error would fail every scheduler tick on such a machine
    // and silently disable reminders altogether. JSON we can't parse from a
    // `pactl` that *did* answer fails loudly.
    let Some(sources) = run("pactl", &["-f", "json", "list", "sources"]) else {
        return Ok(false);
    };
    let Some(source_outputs) = run("pactl", &["-f", "json", "list", "source-outputs"]) else {
        return Ok(false);
    };
    parse_pactl_capturing(&sources, &source_outputs)
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

    // Real `pactl -f json` output from a PipeWire machine running EasyEffects,
    // trimmed to a few entries (volumes and nested fields kept as captured).
    // Source 52 is a sink monitor; 53 is EasyEffects' virtual microphone.
    const REAL_SOURCES: &str = r#"[
  {
    "index": 52,
    "state": "RUNNING",
    "name": "easyeffects_sink.monitor",
    "description": "Monitor of Easy Effects Sink",
    "driver": "PipeWire",
    "owner_module": 4294967295,
    "mute": false,
    "volume": {
      "front-left": {
        "value": 65536,
        "value_percent": "100%",
        "db": "0.00 dB"
      },
      "front-right": {
        "value": 65536,
        "value_percent": "100%",
        "db": "0.00 dB"
      }
    },
    "balance": 0.0,
    "monitor_source": "easyeffects_sink",
    "properties": {
      "application.id": "com.github.wwmm.easyeffects",
      "media.class": "Audio/Sink",
      "node.name": "easyeffects_sink",
      "device.description": "Easy Effects Sink",
      "node.virtual": "true",
      "device.class": "monitor"
    },
    "active_port": null
  },
  {
    "index": 53,
    "state": "IDLE",
    "name": "easyeffects_source",
    "description": "Easy Effects Source",
    "driver": "PipeWire",
    "owner_module": 4294967295,
    "mute": false,
    "volume": {
      "front-left": {
        "value": 65536,
        "value_percent": "100%",
        "db": "0.00 dB"
      },
      "front-right": {
        "value": 65536,
        "value_percent": "100%",
        "db": "0.00 dB"
      }
    },
    "balance": 0.0,
    "monitor_source": "",
    "properties": {
      "application.id": "com.github.wwmm.easyeffects",
      "media.class": "Audio/Source/Virtual",
      "node.name": "easyeffects_source",
      "device.description": "Easy Effects Source",
      "node.virtual": "true"
    },
    "active_port": null
  },
  {
    "index": 86,
    "state": "RUNNING",
    "name": "alsa_input.usb-Focusrite_Scarlett_2i4_USB-00.HiFi__Mic2__source",
    "description": "Scarlett 2i4 Input 2 Mic/Inst/Line",
    "driver": "PipeWire",
    "owner_module": 4294967295,
    "mute": false,
    "volume": {
      "mono": {
        "value": 65536,
        "value_percent": "100%",
        "db": "0.00 dB"
      }
    },
    "balance": 0.0,
    "monitor_source": "",
    "properties": {
      "node.virtual": "false",
      "node.name": "alsa_input.usb-Focusrite_Scarlett_2i4_USB-00.HiFi__Mic2__source",
      "media.class": "Audio/Source",
      "device.description": "Scarlett 2i4",
      "device.class": "sound"
    },
    "active_port": "[In] Mic2"
  }
]"#;

    // A real `pw-record /dev/null` stream, captured while it recorded from
    // source 53 (the default, EasyEffects' virtual microphone).
    const REAL_RECORDING_OUTPUTS: &str = r#"[
  {
    "index": 574,
    "driver": "PipeWire",
    "owner_module": null,
    "client": "573",
    "source": 53,
    "sample_specification": "s16le 2ch 48000Hz",
    "channel_map": "front-left,front-right",
    "corked": false,
    "mute": false,
    "volume": {
      "front-left": {
        "value": 65536,
        "value_percent": "100%",
        "db": "0.00 dB"
      },
      "front-right": {
        "value": 65536,
        "value_percent": "100%",
        "db": "0.00 dB"
      }
    },
    "balance": 0.0,
    "buffer_latency_usec": 0.0,
    "source_latency_usec": 0.0,
    "resample_method": "PipeWire",
    "properties": {
      "application.name": "pw-record",
      "node.name": "pw-record",
      "media.category": "Capture",
      "media.role": "music",
      "media.name": "/dev/null",
      "media.class": "Stream/Input/Audio"
    }
  }
]"#;

    // A real `parecord -d easyeffects_sink.monitor` stream — a desktop-audio
    // recorder reading source 52, a sink monitor.
    const REAL_MONITOR_RECORDING_OUTPUTS: &str = r#"[
  {
    "index": 642,
    "driver": "PipeWire",
    "owner_module": null,
    "client": "641",
    "source": 52,
    "sample_specification": "s16le 2ch 44100Hz",
    "channel_map": "front-left,front-right",
    "corked": false,
    "mute": false,
    "volume": {
      "front-left": {
        "value": 65536,
        "value_percent": "100%",
        "db": "0.00 dB"
      },
      "front-right": {
        "value": 65536,
        "value_percent": "100%",
        "db": "0.00 dB"
      }
    },
    "balance": 0.0,
    "buffer_latency_usec": 0.0,
    "source_latency_usec": 0.0,
    "resample_method": "PipeWire",
    "properties": {
      "application.name": "parecord",
      "media.name": "/dev/null",
      "target.object": "easyeffects_sink",
      "stream.capture.sink": "true",
      "node.name": "parecord",
      "media.class": "Stream/Input/Audio"
    }
  }
]"#;

    /// A minimal source-output reading source `source`, with extra `properties`
    /// given as a JSON object body.
    fn source_output(source: u32, corked: bool, properties: &str) -> String {
        format!(
            r#"[{{ "index": 1, "source": {source}, "corked": {corked}, "properties": {{ {properties} }} }}]"#
        )
    }

    #[test]
    fn a_real_recording_stream_on_a_real_microphone_means_the_mic_is_in_use() {
        // Given real pactl output where pw-record reads easyeffects_source
        // When classified
        // Then the mic is in use
        assert!(parse_pactl_capturing(REAL_SOURCES, REAL_RECORDING_OUTPUTS).expect("parses"));
    }

    #[test]
    fn no_source_outputs_means_the_mic_is_not_in_use() {
        // Given sources that include a RUNNING always-on reader but no recording streams
        // When classified
        // Then the mic is not in use — source state alone is not evidence of a call
        assert!(!parse_pactl_capturing(REAL_SOURCES, "[]").expect("parses"));
    }

    #[test]
    fn a_corked_source_output_is_not_recording() {
        // Given a recording stream that is paused (corked)
        let outputs = source_output(53, true, r#""application.name": "zoom""#);

        // When classified
        // Then the mic is not in use
        assert!(!parse_pactl_capturing(REAL_SOURCES, &outputs).expect("parses"));
    }

    #[test]
    fn a_stream_recording_a_sink_monitor_is_desktop_audio_not_a_call() {
        // Given a real recorder reading a sink monitor
        // When classified
        // Then the mic is not in use
        assert!(
            !parse_pactl_capturing(REAL_SOURCES, REAL_MONITOR_RECORDING_OUTPUTS).expect("parses")
        );
    }

    #[test]
    fn pavucontrols_peak_detect_level_meter_is_not_a_call() {
        // Given pavucontrol's level-meter stream on a real microphone
        let outputs = source_output(
            53,
            false,
            r#""application.id": "org.PulseAudio.pavucontrol", "media.name": "Peak detect""#,
        );

        // When classified
        // Then the mic is not in use
        assert!(!parse_pactl_capturing(REAL_SOURCES, &outputs).expect("parses"));
    }

    #[test]
    fn a_pipewire_stream_monitor_peak_stream_is_ignored() {
        // Given a level-meter stream PipeWire marks with stream.monitor
        let outputs = source_output(
            53,
            false,
            r#""application.name": "some-meter", "stream.monitor": "true""#,
        );

        // When classified
        // Then the mic is not in use
        assert!(!parse_pactl_capturing(REAL_SOURCES, &outputs).expect("parses"));
    }

    #[test]
    fn easyeffects_own_stream_is_ignored_but_another_app_recording_it_counts() {
        // Given EasyEffects reading the hardware mic, with nobody else recording
        let own = source_output(
            86,
            false,
            r#""application.id": "com.github.wwmm.easyeffects""#,
        );

        // When classified
        // Then the mic is not in use
        assert!(!parse_pactl_capturing(REAL_SOURCES, &own).expect("parses"));

        // And when another app records easyeffects_source as well, it is
        let both = format!(
            "{},{}",
            own.trim_end_matches(']'),
            REAL_RECORDING_OUTPUTS.trim().trim_start_matches('[')
        );
        assert!(parse_pactl_capturing(REAL_SOURCES, &both).expect("parses"));
    }

    #[test]
    fn json_that_is_not_an_array_fails_loudly_in_either_call() {
        // Given a top-level object in place of the source or source-output list
        // When classified
        // Then it errors instead of silently reporting the mic as free
        assert!(matches!(
            parse_pactl_capturing("{}", "[]"),
            Err(QuietOsError::UnparsableMic(_))
        ));
        assert!(matches!(
            parse_pactl_capturing("[]", r#"{ "error": "x" }"#),
            Err(QuietOsError::UnparsableMic(_))
        ));
    }

    #[test]
    fn malformed_pactl_json_fails_loudly_rather_than_assuming_the_user_is_free() {
        // Given output that is not valid JSON
        let result = parse_pactl_capturing("not json", "[]");

        // Then it errors instead of silently returning "not in use"
        assert!(matches!(
            result,
            Err(QuietOsError::Json { probe: "mic", .. })
        ));
    }

    #[cfg(target_os = "linux")]
    fn pactl_runner<'a>(
        sources: Option<&'a str>,
        outputs: Option<&'a str>,
    ) -> impl Fn(&str, &[&str]) -> Option<String> + 'a {
        move |program, args| {
            assert_eq!(program, "pactl");
            let answer = if args.last() == Some(&"sources") {
                sources
            } else {
                outputs
            };
            answer.map(str::to_string)
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_probe_combines_both_pactl_answers() {
        // Given pactl answers both calls with a recording stream on a microphone
        let run = pactl_runner(Some(REAL_SOURCES), Some(REAL_RECORDING_OUTPUTS));

        // When probed
        // Then the mic is in use
        assert!(probe_mic_in_use_with(run).expect("probes"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_probe_reports_not_in_use_when_either_pactl_call_is_unavailable() {
        // Given pactl fails for one call or the other (or both)
        for (sources, outputs) in [
            (None, Some(REAL_RECORDING_OUTPUTS)),
            (Some(REAL_SOURCES), None),
            (None, None),
        ] {
            // When probed
            let result = probe_mic_in_use_with(pactl_runner(sources, outputs));

            // Then it degrades to not in use rather than erroring
            assert!(!result.expect("probes"));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_probe_fails_loudly_when_pactl_answers_with_garbage() {
        // Given pactl answers but with something that is not JSON
        let run = pactl_runner(Some("banana"), Some("[]"));

        // When probed
        // Then it errors instead of assuming the user is free
        assert!(matches!(
            probe_mic_in_use_with(run),
            Err(QuietOsError::Json { .. })
        ));
    }
}
