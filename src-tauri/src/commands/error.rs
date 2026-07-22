//! Errors surfaced by the command layer. Kept small and specific so the
//! frontend gets a useful message rather than a silently swallowed failure.

use thiserror::Error;

use crate::domain::DomainError;
use crate::quiet_os::QuietOsError;
use crate::scheduler::SchedulerError;
use crate::store::StoreError;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error(transparent)]
    Store(#[from] StoreError),

    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error(transparent)]
    Scheduler(#[from] SchedulerError),

    #[error(transparent)]
    QuietOs(#[from] QuietOsError),

    #[error("no habit found with id {0}")]
    HabitNotFound(i64),

    #[error("rotation-member habit {0} is missing its rotation_id")]
    MissingRotationId(i64),

    #[error("rotation {0} uses an own window but is missing its start/end bounds")]
    MissingWindowBounds(i64),

    #[error("rotation interval_secs {0} does not fit a u32")]
    IntervalOutOfRange(i64),

    #[error("app config has not been written yet")]
    ConfigNotSet,

    #[error("invalid pause request: {0}")]
    InvalidPauseRequest(String),

    #[error("app state lock was poisoned by a prior panic")]
    StatePoisoned,
}

/// Tauri commands need their error type to serialise across IPC — rendered
/// as the error's display message rather than a structured payload, since
/// the frontend only ever surfaces it as text.
impl serde::Serialize for CommandError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
