//! Errors from the OS quiet probes (design spec §8/§9.2). The probes are the
//! thin impure edge that feeds `QuietState`; when the OS won't answer — a
//! missing tool, denied calendar permission, unparsable output — they fail
//! loudly rather than pretending the user is available.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum QuietOsError {
    #[error("failed to run the {probe} probe: {source}")]
    Spawn {
        probe: &'static str,
        source: std::io::Error,
    },

    #[error("the {probe} probe exited with status {status}: {stderr}")]
    ProbeFailed {
        probe: &'static str,
        status: String,
        stderr: String,
    },

    #[error("could not parse the idle-time probe output: {0}")]
    UnparsableIdle(String),

    #[error("could not read the Do Not Disturb state: {0}")]
    Dnd(#[from] std::io::Error),

    #[error("could not parse the {probe} probe output as JSON: {source}")]
    Json {
        probe: &'static str,
        source: serde_json::Error,
    },

    #[error("could not parse a calendar event timestamp {value:?}: {source}")]
    UnparsableEventTime {
        value: String,
        source: chrono::ParseError,
    },

    #[error("OS quiet probes are only implemented on macOS")]
    Unsupported,
}
