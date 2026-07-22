//! The scheduler's input types: domain `Habit`s paired with the ids the
//! caller uses to track them, grouped the way the pure-scheduler contract
//! expects (design spec §4.6) — rotation members inside their `Rotation`,
//! and schedule-triggered habits as a flat list.

use crate::domain::{Habit, RotationWindow, Trigger};

use super::error::SchedulerError;
use super::ids::{HabitId, RotationId};

/// One member of a rotation, paired with its id (design spec §4.3).
/// Construction is validated: a member's trigger must be `RotationMember`.
#[derive(Debug, Clone, PartialEq)]
pub struct RotationMember {
    pub habit_id: HabitId,
    pub habit: Habit,
}

impl RotationMember {
    /// Builds a rotation member, rejecting a habit whose trigger isn't
    /// `RotationMember`.
    pub fn new(habit_id: HabitId, habit: Habit) -> Result<Self, SchedulerError> {
        match &habit.trigger {
            Trigger::RotationMember { .. } => Ok(Self { habit_id, habit }),
            _ => Err(SchedulerError::NotARotationMember(habit_id)),
        }
    }

    /// The rotation-picker weight backing this member — present because
    /// `new` already verified the trigger is `RotationMember`.
    pub fn weight(&self) -> u32 {
        match self.habit.trigger {
            Trigger::RotationMember { weight } => weight,
            _ => unreachable!("RotationMember::new validates the trigger shape"),
        }
    }
}

/// A rotation (design spec §4.3): interval + window + members, identified so
/// `SchedulerState` can track its last-shown member across calls.
#[derive(Debug, Clone, PartialEq)]
pub struct RotationInput {
    pub id: RotationId,
    pub interval_secs: u32,
    pub window: RotationWindow,
    pub members: Vec<RotationMember>,
}

/// A schedule-triggered habit — either at-time or weekly-count (design spec
/// §4.2), identified so `SchedulerState` can track its firing history.
/// Construction is validated: its trigger must be `Schedule`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledHabit {
    pub id: HabitId,
    pub habit: Habit,
}

impl ScheduledHabit {
    /// Builds a scheduled habit, rejecting a habit whose trigger isn't
    /// `Schedule`.
    pub fn new(id: HabitId, habit: Habit) -> Result<Self, SchedulerError> {
        match &habit.trigger {
            Trigger::Schedule(_) => Ok(Self { id, habit }),
            _ => Err(SchedulerError::NotAScheduleTrigger(id)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Recurrence, TimeOfDay};
    use crate::store::Category;

    fn rotation_member_habit() -> Habit {
        Habit::new(
            "Lunge-and-reach".to_string(),
            "5 slow reps/leg".to_string(),
            None,
            Category::Exercise,
            true,
            Trigger::rotation_member(2).expect("valid weight"),
        )
        .expect("valid habit")
    }

    fn scheduled_habit() -> Habit {
        let time = TimeOfDay::new(9, 0).expect("valid time");
        Habit::new(
            "Morning stretch".to_string(),
            "Full-body stretch".to_string(),
            None,
            Category::General,
            true,
            Trigger::at_time(time, Recurrence::Daily, true).expect("valid trigger"),
        )
        .expect("valid habit")
    }

    #[test]
    fn a_rotation_member_habit_builds_and_reports_its_weight() {
        // Given a habit whose trigger is RotationMember{weight: 2}
        // When building a RotationMember from it
        let member = RotationMember::new(HabitId(1), rotation_member_habit()).expect("builds");

        // Then it reports the weight from the trigger
        assert_eq!(member.weight(), 2);
    }

    #[test]
    fn a_scheduled_habit_used_as_a_rotation_member_is_rejected() {
        // Given a habit with a Schedule trigger, not RotationMember
        // When building a RotationMember from it
        let result = RotationMember::new(HabitId(1), scheduled_habit());

        // Then it fails loudly rather than silently accepting a mismatch
        assert_eq!(result, Err(SchedulerError::NotARotationMember(HabitId(1))));
    }

    #[test]
    fn a_rotation_member_habit_used_as_a_scheduled_habit_is_rejected() {
        // Given a habit with a RotationMember trigger, not Schedule
        // When building a ScheduledHabit from it
        let result = ScheduledHabit::new(HabitId(1), rotation_member_habit());

        // Then it fails loudly rather than silently accepting a mismatch
        assert_eq!(result, Err(SchedulerError::NotAScheduleTrigger(HabitId(1))));
    }

    #[test]
    fn a_schedule_triggered_habit_builds_as_a_scheduled_habit() {
        // Given a habit with a Schedule trigger
        // When building a ScheduledHabit from it
        let result = ScheduledHabit::new(HabitId(1), scheduled_habit());

        // Then it succeeds
        assert!(result.is_ok());
    }
}
