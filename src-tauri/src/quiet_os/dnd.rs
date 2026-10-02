//! Do Not Disturb / Focus probe (design spec §4.5 / §3.6). Modern macOS records
//! active Focus modes in a local assertions file; a non-empty assertion record
//! means a Focus (including plain Do Not Disturb) is currently on. On Linux
//! (GNOME) Do Not Disturb is the `show-banners` notification setting, read via
//! `gsettings`: `false` means banners are suppressed, i.e. DND is on. Reading
//! the file / running `gsettings` is the thin impure wrapper; the
//! interpretation is pure and unit-tested. Fully local — no network, no
//! private API.

#[cfg(target_os = "macos")]
use std::path::PathBuf;

use serde_json::Value;

use super::error::QuietOsError;
#[cfg(target_os = "linux")]
use super::tool_output::run_stdout_if_available;

/// The per-user file macOS writes active Focus assertions into, relative to the
/// home directory.
#[cfg(target_os = "macos")]
const ASSERTIONS_PATH: &str = "Library/DoNotDisturb/DB/Assertions.json";

/// Whether a Focus / Do Not Disturb mode is active, given the raw contents of
/// the assertions file. A Focus is on when any `data` entry carries at least
/// one `storeAssertionRecords` element.
pub fn parse_focus_active(assertions_json: &str) -> Result<bool, QuietOsError> {
    let parsed: Value =
        serde_json::from_str(assertions_json).map_err(|source| QuietOsError::Json {
            probe: "dnd",
            source,
        })?;
    let active = parsed
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.get("storeAssertionRecords"))
        .filter_map(Value::as_array)
        .any(|records| !records.is_empty());
    Ok(active)
}

/// Whether GNOME Do Not Disturb is active, given the output of
/// `gsettings get org.gnome.desktop.notifications show-banners`. The setting
/// is inverted relative to the question: `false` (banners hidden) means DND is
/// on, `true` means it is off.
pub fn parse_show_banners(gsettings_output: &str) -> Result<bool, QuietOsError> {
    match gsettings_output.trim() {
        "false" => Ok(true),
        "true" => Ok(false),
        other => Err(QuietOsError::UnparsableDnd(other.to_string())),
    }
}

#[cfg(target_os = "macos")]
pub fn probe_dnd() -> Result<bool, QuietOsError> {
    let path = home_dir()?.join(ASSERTIONS_PATH);
    // A missing file is not a failure: it simply means no Focus has ever been
    // configured, so none is active. Any other read error is surfaced loudly.
    match std::fs::read_to_string(&path) {
        Ok(contents) => parse_focus_active(&contents),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(QuietOsError::Dnd(err)),
    }
}

#[cfg(target_os = "macos")]
fn home_dir() -> Result<PathBuf, QuietOsError> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| QuietOsError::Dnd(std::io::Error::other("HOME is not set")))
}

#[cfg(target_os = "linux")]
pub fn probe_dnd() -> Result<bool, QuietOsError> {
    probe_dnd_with(run_stdout_if_available)
}

/// The Linux DND probe with the tool runner injected, so the degrade and
/// error handling can be tested without a desktop session.
#[cfg(target_os = "linux")]
fn probe_dnd_with(run: impl Fn(&str, &[&str]) -> Option<String>) -> Result<bool, QuietOsError> {
    // A missing `gsettings` or an absent schema (non-GNOME desktop) exits
    // nonzero or fails to spawn; that degrades to "no DND" rather than
    // erroring, since an unconditional error would fail every scheduler tick
    // on such a machine and silently disable reminders altogether. A value we
    // can't parse from a `gsettings` that *did* answer fails loudly.
    match run(
        "gsettings",
        &["get", "org.gnome.desktop.notifications", "show-banners"],
    ) {
        Some(output) => parse_show_banners(&output),
        None => Ok(false),
    }
}

// Not implemented on this platform. Degrades to "no Focus active" —
// consistent with `calendar::list_day_meetings` — rather than erroring, since
// an unconditional error here would fail every scheduler tick forever and
// silently disable reminders altogether.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn probe_dnd() -> Result<bool, QuietOsError> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_empty_assertion_record_means_a_focus_is_active() {
        // Given assertions JSON with one active Focus assertion
        let json = r#"{
            "data": [
                {
                    "storeAssertionRecords": [
                        { "assertionDetails": { "assertionDetailsModeIdentifier": "com.apple.donotdisturb.mode.default" } }
                    ]
                }
            ]
        }"#;

        // When interpreted
        // Then a Focus is reported as active
        assert!(parse_focus_active(json).expect("parses"));
    }

    #[test]
    fn an_empty_assertions_list_means_no_focus_is_active() {
        // Given assertions JSON where the record list is empty (Focus off)
        let json = r#"{ "data": [ { "storeAssertionRecords": [] } ] }"#;

        // When interpreted
        // Then no Focus is active
        assert!(!parse_focus_active(json).expect("parses"));
    }

    #[test]
    fn assertions_json_missing_the_records_key_entirely_means_no_focus() {
        // Given assertions JSON with a data entry but no records key
        let json = r#"{ "data": [ {} ] }"#;

        // When interpreted
        // Then no Focus is active
        assert!(!parse_focus_active(json).expect("parses"));
    }

    #[test]
    fn malformed_json_fails_loudly_rather_than_assuming_the_user_is_free() {
        // Given contents that are not valid JSON
        // When interpreted
        let result = parse_focus_active("not json at all");

        // Then it errors instead of silently returning "no Focus"
        assert!(matches!(result, Err(QuietOsError::Json { .. })));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    #[test]
    fn on_unsupported_platforms_the_probe_degrades_to_not_active_rather_than_erroring() {
        // Given a platform with no DND probe implementation
        // When probed
        // Then it reports no Focus active instead of failing the scheduler tick
        assert!(matches!(probe_dnd(), Ok(false)));
    }

    #[test]
    fn show_banners_false_means_do_not_disturb_is_on() {
        // Given gsettings reports banners are hidden
        // When interpreted
        // Then Do Not Disturb is active
        assert!(parse_show_banners("false\n").expect("parses"));
    }

    #[test]
    fn show_banners_true_means_do_not_disturb_is_off() {
        // Given gsettings reports banners are shown, with stray whitespace
        // When interpreted
        // Then Do Not Disturb is not active
        assert!(!parse_show_banners("  true \n").expect("parses"));
    }

    #[test]
    fn unrecognised_show_banners_output_fails_loudly() {
        // Given output that is neither "true" nor "false"
        let result = parse_show_banners("uint32 1");

        // Then it errors instead of silently returning "not disturbed"
        assert!(matches!(result, Err(QuietOsError::UnparsableDnd(_))));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_probe_reads_the_show_banners_setting() {
        // Given gsettings answers that banners are hidden
        // When probed
        // Then Do Not Disturb is on
        assert!(probe_dnd_with(|_, _| Some("false\n".to_string())).expect("probes"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_probe_reports_no_dnd_when_gsettings_is_unavailable() {
        // Given gsettings is missing or has no such schema
        // When probed
        // Then it degrades to no Do Not Disturb rather than erroring
        assert!(!probe_dnd_with(|_, _| None).expect("probes"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_probe_fails_loudly_when_gsettings_answers_with_garbage() {
        // Given gsettings answers with an unrecognised value
        // When probed
        let result = probe_dnd_with(|_, _| Some("uint32 1".to_string()));

        // Then it errors instead of assuming the user is free
        assert!(matches!(result, Err(QuietOsError::UnparsableDnd(_))));
    }
}
