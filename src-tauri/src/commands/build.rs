//! Builds the scheduler's pure input types (`ScheduledHabit`/`RotationInput`)
//! from the store's flat rows — the only place command handlers reach across
//! the store/domain/scheduler boundary, so the rest of the commands module
//! stays thin.

use std::collections::HashMap;
use std::str::FromStr;

use crate::domain::{Habit, RotationWindow, TimeOfDay, TimeWindow, Trigger};
use crate::scheduler::{HabitId, RotationId, RotationInput, RotationMember, ScheduledHabit};
use crate::store::{self, Store, WindowKind};

use super::error::CommandError;

/// Builds every schedule-triggered habit and every rotation (with its
/// members) the scheduler needs to consider. Disabled habits are excluded —
/// matching the store's "disabled habits are excluded from scheduling"
/// contract (see `Store::disable_habit`).
pub fn build_scheduler_inputs(
    store: &Store,
) -> Result<(Vec<ScheduledHabit>, Vec<RotationInput>), CommandError> {
    let habit_rows = store.list_habits()?;
    let mut scheduled = Vec::new();
    let mut members_by_rotation: HashMap<i64, Vec<RotationMember>> = HashMap::new();

    for row in habit_rows.iter().filter(|row| row.enabled) {
        let habit = Habit::from_store(row)?;
        match &habit.trigger {
            Trigger::RotationMember { .. } => {
                let rotation_id = row
                    .rotation_id
                    .ok_or(CommandError::MissingRotationId(row.id))?;
                members_by_rotation
                    .entry(rotation_id)
                    .or_default()
                    .push(RotationMember::new(HabitId(row.id), habit)?);
            }
            Trigger::Schedule(_) => {
                scheduled.push(ScheduledHabit::new(HabitId(row.id), habit)?);
            }
        }
    }

    let rotations = build_rotation_inputs(store, members_by_rotation)?;
    Ok((scheduled, rotations))
}

/// Pairs each rotation row with its (already-grouped) enabled members,
/// skipping rotations that ended up with none.
fn build_rotation_inputs(
    store: &Store,
    mut members_by_rotation: HashMap<i64, Vec<RotationMember>>,
) -> Result<Vec<RotationInput>, CommandError> {
    let mut rotations = Vec::new();
    for row in store.list_rotations()? {
        let Some(members) = members_by_rotation.remove(&row.id) else {
            continue;
        };
        rotations.push(RotationInput {
            id: RotationId(row.id),
            interval_secs: u32::try_from(row.interval_secs)
                .map_err(|_| CommandError::IntervalOutOfRange(row.interval_secs))?,
            window: rotation_window_from_row(&row)?,
            members,
        });
    }
    Ok(rotations)
}

/// Converts a rotation row's `window_kind` + optional bounds into the
/// scheduler's `RotationWindow`.
fn rotation_window_from_row(row: &store::Rotation) -> Result<RotationWindow, CommandError> {
    match row.window_kind {
        WindowKind::AlwaysOn => Ok(RotationWindow::AlwaysOn),
        WindowKind::InheritGlobal => Ok(RotationWindow::InheritGlobal),
        WindowKind::Own => {
            let start = row
                .window_start
                .as_deref()
                .ok_or(CommandError::MissingWindowBounds(row.id))?;
            let end = row
                .window_end
                .as_deref()
                .ok_or(CommandError::MissingWindowBounds(row.id))?;
            let window = TimeWindow::new(TimeOfDay::from_str(start)?, TimeOfDay::from_str(end)?)?;
            Ok(RotationWindow::Own(window))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Category, NewHabit, NewRotation, TriggerKind};

    fn rotation_member(name: &str, weight: i64, rotation_id: Option<i64>) -> NewHabit {
        NewHabit {
            name: name.to_string(),
            instructions: "instructions".to_string(),
            media_path: None,
            category: Category::Exercise,
            enabled: true,
            trigger_kind: TriggerKind::RotationMember,
            trigger_config_json: "{}".to_string(),
            weight: Some(weight),
            rotation_id,
            created_at: 0,
        }
    }

    #[test]
    fn enabled_rotation_members_are_grouped_under_their_rotation() {
        // Given a rotation with two enabled members
        let store = Store::open_in_memory().expect("in-memory store opens");
        let rotation_id = store
            .insert_rotation(&NewRotation {
                name: "Movement snacks".to_string(),
                interval_secs: 1_800,
                window_kind: WindowKind::InheritGlobal,
                window_start: None,
                window_end: None,
            })
            .expect("rotation insert succeeds");
        store
            .insert_habit(&rotation_member("Lunge-and-reach", 2, Some(rotation_id)))
            .expect("insert succeeds");
        store
            .insert_habit(&rotation_member("Glute bridges", 1, Some(rotation_id)))
            .expect("insert succeeds");

        // When building the scheduler inputs
        let (scheduled, rotations) = build_scheduler_inputs(&store).expect("builds");

        // Then no schedule-triggered habits exist, and the rotation carries both members
        assert!(scheduled.is_empty());
        assert_eq!(rotations.len(), 1);
        assert_eq!(rotations[0].members.len(), 2);
    }

    #[test]
    fn a_disabled_rotation_member_is_excluded() {
        // Given a rotation with one enabled and one disabled member
        let store = Store::open_in_memory().expect("in-memory store opens");
        let rotation_id = store
            .insert_rotation(&NewRotation {
                name: "Movement snacks".to_string(),
                interval_secs: 1_800,
                window_kind: WindowKind::InheritGlobal,
                window_start: None,
                window_end: None,
            })
            .expect("rotation insert succeeds");
        store
            .insert_habit(&rotation_member("Lunge-and-reach", 2, Some(rotation_id)))
            .expect("insert succeeds");
        let disabled_id = store
            .insert_habit(&rotation_member("Glute bridges", 1, Some(rotation_id)))
            .expect("insert succeeds");
        let mut disabled = store.list_habits().expect("list succeeds");
        let mut disabled_row = disabled.remove(
            disabled
                .iter()
                .position(|h| h.id == disabled_id)
                .expect("row present"),
        );
        disabled_row.enabled = false;
        store.update_habit(&disabled_row).expect("update succeeds");

        // When building the scheduler inputs
        let (_, rotations) = build_scheduler_inputs(&store).expect("builds");

        // Then only the enabled member is carried through
        assert_eq!(rotations[0].members.len(), 1);
        assert_eq!(rotations[0].members[0].habit.name, "Lunge-and-reach");
    }

    #[test]
    fn a_rotation_with_no_enabled_members_is_omitted_entirely() {
        // Given a rotation whose only member is disabled
        let store = Store::open_in_memory().expect("in-memory store opens");
        let rotation_id = store
            .insert_rotation(&NewRotation {
                name: "Movement snacks".to_string(),
                interval_secs: 1_800,
                window_kind: WindowKind::InheritGlobal,
                window_start: None,
                window_end: None,
            })
            .expect("rotation insert succeeds");
        let habit_id = store
            .insert_habit(&rotation_member("Lunge-and-reach", 2, Some(rotation_id)))
            .expect("insert succeeds");
        let mut row = store.list_habits().expect("list succeeds").remove(0);
        assert_eq!(row.id, habit_id);
        row.enabled = false;
        store.update_habit(&row).expect("update succeeds");

        // When building the scheduler inputs
        let (_, rotations) = build_scheduler_inputs(&store).expect("builds");

        // Then the now-empty rotation is dropped rather than passed through empty
        assert!(rotations.is_empty());
    }

    #[test]
    fn a_schedule_triggered_habit_is_returned_as_scheduled_not_grouped_into_a_rotation() {
        // Given a daily at-time habit
        let store = Store::open_in_memory().expect("in-memory store opens");
        store
            .insert_habit(&NewHabit {
                name: "Morning stretch".to_string(),
                instructions: "Full-body stretch".to_string(),
                media_path: None,
                category: Category::General,
                enabled: true,
                trigger_kind: TriggerKind::ScheduleAtTime,
                trigger_config_json:
                    r#"{"time":{"hour":9,"minute":0},"recurrence":{"kind":"daily"},"expires_at_day_end":true}"#
                        .to_string(),
                weight: None,
                rotation_id: None,
                created_at: 0,
            })
            .expect("insert succeeds");

        // When building the scheduler inputs
        let (scheduled, rotations) = build_scheduler_inputs(&store).expect("builds");

        // Then it appears as a scheduled habit, and no rotation is produced
        assert_eq!(scheduled.len(), 1);
        assert!(rotations.is_empty());
    }

    #[test]
    fn a_rotation_member_missing_its_rotation_id_fails_loudly() {
        // Given a rotation-member habit row with no rotation_id — a data
        // integrity problem, not a normal absence
        let store = Store::open_in_memory().expect("in-memory store opens");
        store
            .insert_habit(&rotation_member("Orphan", 1, None))
            .expect("insert succeeds");

        // When building the scheduler inputs
        let result = build_scheduler_inputs(&store);

        // Then it fails loudly rather than silently dropping the habit
        assert!(matches!(result, Err(CommandError::MissingRotationId(_))));
    }

    #[test]
    fn an_own_window_rotation_round_trips_its_bounds() {
        // Given a rotation with its own window
        let store = Store::open_in_memory().expect("in-memory store opens");
        let rotation_id = store
            .insert_rotation(&NewRotation {
                name: "Early birds".to_string(),
                interval_secs: 600,
                window_kind: WindowKind::Own,
                window_start: Some("07:00".to_string()),
                window_end: Some("08:30".to_string()),
            })
            .expect("rotation insert succeeds");
        store
            .insert_habit(&rotation_member("Wall sit", 1, Some(rotation_id)))
            .expect("insert succeeds");

        // When building the scheduler inputs
        let (_, rotations) = build_scheduler_inputs(&store).expect("builds");

        // Then its window bounds are preserved
        assert_eq!(
            rotations[0].window,
            RotationWindow::Own(
                TimeWindow::new(
                    TimeOfDay::new(7, 0).expect("valid time"),
                    TimeOfDay::new(8, 30).expect("valid time"),
                )
                .expect("valid window")
            )
        );
    }

    #[test]
    fn an_own_window_rotation_missing_its_bounds_fails_loudly() {
        // Given a rotation row claiming an own window but with no bounds set —
        // a data integrity problem the CHECK constraints don't catch
        let store = Store::open_in_memory().expect("in-memory store opens");
        let rotation_id = store
            .insert_rotation(&NewRotation {
                name: "Early birds".to_string(),
                interval_secs: 600,
                window_kind: WindowKind::Own,
                window_start: None,
                window_end: None,
            })
            .expect("rotation insert succeeds");
        store
            .insert_habit(&rotation_member("Wall sit", 1, Some(rotation_id)))
            .expect("insert succeeds");

        // When building the scheduler inputs
        let result = build_scheduler_inputs(&store);

        // Then it fails loudly rather than silently defaulting a window
        assert!(matches!(result, Err(CommandError::MissingWindowBounds(_))));
    }
}
