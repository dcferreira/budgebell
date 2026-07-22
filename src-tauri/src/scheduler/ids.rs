//! Identifiers the caller assigns to habits and rotations so the scheduler
//! can correlate `last_shown_state` across calls without depending on the
//! store — the domain `Habit`/`Rotation` types carry no id of their own,
//! since ids are a storage-layer concern.

/// Identifies a habit across scheduler calls. Mirrors the store's row id
/// (`i64`) so a later `commands` task can plug store ids straight through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct HabitId(pub i64);

/// Identifies a rotation across scheduler calls — see [`HabitId`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RotationId(pub i64);
