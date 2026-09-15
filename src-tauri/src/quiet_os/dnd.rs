//! Do Not Disturb / Focus probe (design spec §4.5 / §3.6). Modern macOS records
//! active Focus modes in a local assertions file; a non-empty assertion record
//! means a Focus (including plain Do Not Disturb) is currently on. Reading the
//! file is the thin impure wrapper; the JSON interpretation is pure and
//! unit-tested. Fully local — no network, no private API.

#[cfg(target_os = "macos")]
use std::path::PathBuf;

use serde_json::Value;

use super::error::QuietOsError;

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

// Not yet implemented on this platform. Degrades to "no Focus active" —
// consistent with `calendar::list_day_meetings` — rather than erroring, since
// an unconditional error here would fail every scheduler tick forever and
// silently disable reminders altogether.
#[cfg(not(target_os = "macos"))]
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

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn on_non_macos_the_probe_degrades_to_not_active_rather_than_erroring() {
        // Given a platform with no DND probe implementation
        // When probed
        // Then it reports no Focus active instead of failing the scheduler tick
        assert!(matches!(probe_dnd(), Ok(false)));
    }
}
