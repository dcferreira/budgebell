use crate::store;
use crate::store::Category;

use super::error::DomainError;
use super::trigger::Trigger;

/// A habit: content plus exactly one trigger (design spec §4.1/§4.2). Unlike
/// the store's flat row (nullable `weight`/`rotation_id` columns alongside a
/// `trigger_kind` discriminant), the trigger is a single field here — the
/// "exactly one trigger" rule is enforced by the type system, not just
/// application logic.
#[derive(Debug, Clone, PartialEq)]
pub struct Habit {
    pub name: String,
    pub instructions: String,
    pub media_path: Option<String>,
    pub category: Category,
    pub enabled: bool,
    pub trigger: Trigger,
}

impl Habit {
    /// Builds a habit, rejecting empty content fields.
    pub fn new(
        name: String,
        instructions: String,
        media_path: Option<String>,
        category: Category,
        enabled: bool,
        trigger: Trigger,
    ) -> Result<Self, DomainError> {
        if name.trim().is_empty() {
            return Err(DomainError::EmptyName);
        }
        if instructions.trim().is_empty() {
            return Err(DomainError::EmptyInstructions);
        }
        Ok(Self {
            name,
            instructions,
            media_path,
            category,
            enabled,
            trigger,
        })
    }

    /// Converts this habit into the store's insertable row shape, splitting
    /// the trigger into its `trigger_kind` / `trigger_config_json` / `weight`
    /// parts. `rotation_id` is a store-level relationship, supplied by the
    /// caller (e.g. when adding this habit as a member of a specific
    /// rotation), not part of the domain trigger itself.
    pub fn to_new_habit(
        &self,
        created_at: i64,
        rotation_id: Option<i64>,
    ) -> Result<store::NewHabit, DomainError> {
        Ok(store::NewHabit {
            name: self.name.clone(),
            instructions: self.instructions.clone(),
            media_path: self.media_path.clone(),
            category: self.category,
            enabled: self.enabled,
            trigger_kind: self.trigger.kind(),
            trigger_config_json: self.trigger.config_json()?,
            weight: self.trigger.weight_column(),
            rotation_id,
            created_at,
        })
    }

    /// Reconstructs a domain habit from a store row, decoding its trigger
    /// from `trigger_kind` + `trigger_config_json` + `weight`.
    pub fn from_store(row: &store::Habit) -> Result<Self, DomainError> {
        let trigger =
            Trigger::from_store_parts(row.trigger_kind, &row.trigger_config_json, row.weight)?;
        Ok(Self {
            name: row.name.clone(),
            instructions: row.instructions.clone(),
            media_path: row.media_path.clone(),
            category: row.category,
            enabled: row.enabled,
            trigger,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::time::TimeOfDay;
    use crate::domain::trigger::Recurrence;
    use crate::store::Store;

    /// A sane rotation-member habit; tests override only the fields they
    /// care about.
    fn sample_habit() -> Habit {
        Habit::new(
            "Lunge-and-reach".to_string(),
            "5 slow reps/leg, reach overhead".to_string(),
            None,
            Category::Exercise,
            true,
            Trigger::rotation_member(2).expect("valid weight"),
        )
        .expect("valid habit")
    }

    #[test]
    fn a_habit_with_an_empty_name_is_rejected() {
        // Given an empty name
        // When building a habit
        let result = Habit::new(
            "   ".to_string(),
            "instructions".to_string(),
            None,
            Category::General,
            true,
            Trigger::rotation_member(1).expect("valid weight"),
        );

        // Then it fails loudly rather than accepting an unnamed habit
        assert!(matches!(result, Err(DomainError::EmptyName)));
    }

    #[test]
    fn a_habit_with_empty_instructions_is_rejected() {
        // Given empty instructions
        // When building a habit
        let result = Habit::new(
            "Name".to_string(),
            "".to_string(),
            None,
            Category::General,
            true,
            Trigger::rotation_member(1).expect("valid weight"),
        );

        // Then it fails loudly rather than accepting a habit nobody could follow
        assert!(matches!(result, Err(DomainError::EmptyInstructions)));
    }

    #[test]
    fn a_rotation_member_habit_round_trips_through_the_store() {
        // Given a rotation-member habit and an in-memory store
        let store = Store::open_in_memory().expect("in-memory store opens");
        let habit = sample_habit();

        // When it is converted to a store row, inserted, and read back
        let new_habit = habit.to_new_habit(1_700_000_000, None).expect("converts");
        store.insert_habit(&new_habit).expect("insert succeeds");
        let row = store.list_habits().expect("list succeeds").remove(0);
        let rebuilt = Habit::from_store(&row).expect("reconstructs");

        // Then the reconstructed domain habit matches the original exactly
        assert_eq!(rebuilt, habit);
    }

    #[test]
    fn a_scheduled_habit_round_trips_through_the_store() {
        // Given a "09:00 daily, expires at day end" scheduled habit
        let store = Store::open_in_memory().expect("in-memory store opens");
        let time = TimeOfDay::new(9, 0).expect("valid time");
        let habit = Habit::new(
            "Morning stretch".to_string(),
            "Full-body stretch routine".to_string(),
            Some("media/stretch.png".to_string()),
            Category::General,
            true,
            Trigger::at_time(time, Recurrence::Daily, true).expect("valid trigger"),
        )
        .expect("valid habit");

        // When it is converted to a store row, inserted, and read back
        let new_habit = habit.to_new_habit(1_700_000_000, None).expect("converts");
        store.insert_habit(&new_habit).expect("insert succeeds");
        let row = store.list_habits().expect("list succeeds").remove(0);
        let rebuilt = Habit::from_store(&row).expect("reconstructs");

        // Then the reconstructed domain habit matches the original exactly
        assert_eq!(rebuilt, habit);
    }

    #[test]
    fn a_rotation_member_habit_carries_its_rotation_id_via_the_store_row() {
        // Given a rotation and a member habit belonging to it
        let store = Store::open_in_memory().expect("in-memory store opens");
        let rotation_id = store
            .insert_rotation(&store::NewRotation {
                name: "Movement snacks".to_string(),
                interval_secs: 1_800,
                window_kind: store::WindowKind::InheritGlobal,
                window_start: None,
                window_end: None,
            })
            .expect("rotation insert succeeds");
        let habit = sample_habit();

        // When the habit is inserted as a member of that rotation
        let new_habit = habit
            .to_new_habit(1_700_000_000, Some(rotation_id))
            .expect("converts");
        store.insert_habit(&new_habit).expect("insert succeeds");
        let row = store.list_habits().expect("list succeeds").remove(0);

        // Then the row's rotation_id links back to the rotation — a
        // store-level relationship the domain trigger itself doesn't carry
        assert_eq!(row.rotation_id, Some(rotation_id));
        assert_eq!(Habit::from_store(&row).expect("reconstructs"), habit);
    }
}
