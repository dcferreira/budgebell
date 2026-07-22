use thiserror::Error;

use super::ids::HabitId;

/// Errors surfaced when constructing the scheduler's input types. Kept small
/// and specific so callers fail loudly rather than silently mismatching a
/// habit with the wrong trigger shape.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SchedulerError {
    #[error("rotation member habit {0:?} does not have a RotationMember trigger")]
    NotARotationMember(HabitId),

    #[error("scheduled habit {0:?} does not have a Schedule trigger")]
    NotAScheduleTrigger(HabitId),
}
