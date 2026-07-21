use thiserror::Error;

/// Errors surfaced by the domain model's validation and (de)serialisation.
/// Kept small and specific so callers fail loudly with useful context rather
/// than falling back silently.
#[derive(Debug, Error)]
pub enum DomainError {
    #[error("invalid time of day {hour:02}:{minute:02} — hour must be 0-23 and minute 0-59")]
    InvalidTimeOfDay { hour: u32, minute: u32 },

    #[error("could not parse \"{0}\" as a time of day in HH:MM format")]
    UnparsableTimeOfDay(String),

    #[error("a window's start and end must differ")]
    DegenerateTimeWindow,

    #[error("habit name must not be empty")]
    EmptyName,

    #[error("habit instructions must not be empty")]
    EmptyInstructions,

    #[error("a rotation-member weight must be greater than zero")]
    ZeroWeight,

    #[error("weekly-count must be between 1 and 7 times per week, got {0}")]
    WeeklyCountOutOfRange(u8),

    #[error("a specific-weekdays recurrence must list at least one day")]
    EmptySpecificWeekdays,

    #[error("a rotation interval must be greater than zero seconds")]
    ZeroInterval,

    #[error("a rotation must have at least one member")]
    EmptyRotationMembers,

    #[error("every rotation member's trigger must be RotationMember — habit \"{0}\" is not")]
    NonRotationMemberInRotation(String),

    #[error("trigger config (de)serialisation failed: {0}")]
    Json(#[from] serde_json::Error),
}
