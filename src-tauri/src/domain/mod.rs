//! The domain model for the scheduling system (design spec §4): `Habit` and
//! its `Trigger`, `Rotation`, and the globals (`DayConfig`, `QuietState`) the
//! pure scheduler (a later task) will consume. Validation lives here so
//! invalid states (a zero weight, an empty rotation, a malformed schedule)
//! are rejected at construction rather than discovered downstream.
//!
//! This module builds on the store's row-level types (`Category`,
//! `TriggerKind`) and knows how to convert to/from the store's flat
//! `trigger_config_json` representation, but the store never depends back
//! on this module.

mod day_config;
mod error;
mod habit;
mod quiet_state;
mod rotation;
mod time;
mod trigger;

pub use day_config::DayConfig;
pub use error::DomainError;
pub use habit::Habit;
pub use quiet_state::QuietState;
pub use rotation::{Rotation, RotationWindow};
pub use time::{TimeOfDay, TimeWindow, Weekday};
pub use trigger::{AtTimeConfig, Recurrence, ScheduleTrigger, Trigger, WeeklyCountConfig};
