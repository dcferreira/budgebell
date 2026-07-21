use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use rusqlite::{params, Row};

use super::{Store, StoreError};

/// The two content categories a habit can belong to (design spec §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Exercise,
    General,
}

impl Category {
    fn as_str(self) -> &'static str {
        match self {
            Category::Exercise => "exercise",
            Category::General => "general",
        }
    }
}

impl ToSql for Category {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for Category {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "exercise" => Ok(Category::Exercise),
            "general" => Ok(Category::General),
            other => Err(FromSqlError::Other(
                format!("unknown habit category: {other}").into(),
            )),
        }
    }
}

/// Which trigger form a habit uses — exactly one per habit (design spec §4.2).
/// The trigger-specific detail (recurrence, weekdays, N-per-week, preferred
/// time, `expires_at_day_end`, rotation weight) lives in `trigger_config_json`
/// and is interpreted by the domain-model layer, not the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerKind {
    RotationMember,
    ScheduleAtTime,
    ScheduleWeeklyCount,
}

impl TriggerKind {
    fn as_str(self) -> &'static str {
        match self {
            TriggerKind::RotationMember => "rotation-member",
            TriggerKind::ScheduleAtTime => "schedule-at-time",
            TriggerKind::ScheduleWeeklyCount => "schedule-weekly-count",
        }
    }
}

impl ToSql for TriggerKind {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for TriggerKind {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "rotation-member" => Ok(TriggerKind::RotationMember),
            "schedule-at-time" => Ok(TriggerKind::ScheduleAtTime),
            "schedule-weekly-count" => Ok(TriggerKind::ScheduleWeeklyCount),
            other => Err(FromSqlError::Other(
                format!("unknown trigger kind: {other}").into(),
            )),
        }
    }
}

/// A habit as persisted in the store: content plus its trigger metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct Habit {
    pub id: i64,
    pub name: String,
    pub instructions: String,
    pub media_path: Option<String>,
    pub category: Category,
    pub enabled: bool,
    pub trigger_kind: TriggerKind,
    pub trigger_config_json: String,
    pub weight: Option<i64>,
    pub rotation_id: Option<i64>,
    pub created_at: i64,
}

/// Fields required to insert a new habit; the store assigns `id`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewHabit {
    pub name: String,
    pub instructions: String,
    pub media_path: Option<String>,
    pub category: Category,
    pub enabled: bool,
    pub trigger_kind: TriggerKind,
    pub trigger_config_json: String,
    pub weight: Option<i64>,
    pub rotation_id: Option<i64>,
    pub created_at: i64,
}

fn row_to_habit(row: &Row) -> rusqlite::Result<Habit> {
    Ok(Habit {
        id: row.get(0)?,
        name: row.get(1)?,
        instructions: row.get(2)?,
        media_path: row.get(3)?,
        category: row.get(4)?,
        enabled: row.get(5)?,
        trigger_kind: row.get(6)?,
        trigger_config_json: row.get(7)?,
        weight: row.get(8)?,
        rotation_id: row.get(9)?,
        created_at: row.get(10)?,
    })
}

impl Store {
    /// Inserts a new habit, returning the id SQLite assigned to it.
    pub fn insert_habit(&self, habit: &NewHabit) -> Result<i64, StoreError> {
        self.conn.execute(
            "INSERT INTO habits (
                name, instructions, media_path, category, enabled,
                trigger_kind, trigger_config_json, weight, rotation_id, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                habit.name,
                habit.instructions,
                habit.media_path,
                habit.category,
                habit.enabled,
                habit.trigger_kind,
                habit.trigger_config_json,
                habit.weight,
                habit.rotation_id,
                habit.created_at,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Lists every habit — enabled or disabled — ordered by insertion.
    pub fn list_habits(&self) -> Result<Vec<Habit>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, instructions, media_path, category, enabled,
                    trigger_kind, trigger_config_json, weight, rotation_id, created_at
             FROM habits ORDER BY id",
        )?;
        let habits = stmt
            .query_map([], row_to_habit)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(habits)
    }

    /// Replaces a habit's fields in place, keyed by `habit.id`.
    pub fn update_habit(&self, habit: &Habit) -> Result<(), StoreError> {
        let changed = self.conn.execute(
            "UPDATE habits SET
                name = ?1, instructions = ?2, media_path = ?3, category = ?4,
                enabled = ?5, trigger_kind = ?6, trigger_config_json = ?7,
                weight = ?8, rotation_id = ?9
             WHERE id = ?10",
            params![
                habit.name,
                habit.instructions,
                habit.media_path,
                habit.category,
                habit.enabled,
                habit.trigger_kind,
                habit.trigger_config_json,
                habit.weight,
                habit.rotation_id,
                habit.id,
            ],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound { id: habit.id });
        }
        Ok(())
    }

    /// Marks a habit as disabled without deleting its history — disabled
    /// habits are excluded from scheduling but keep their logged events.
    pub fn disable_habit(&self, id: i64) -> Result<(), StoreError> {
        let changed = self
            .conn
            .execute("UPDATE habits SET enabled = 0 WHERE id = ?1", params![id])?;
        if changed == 0 {
            return Err(StoreError::NotFound { id });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    /// A sane rotation-member habit; tests override only the fields they
    /// care about via struct-update syntax.
    fn sample_new_habit() -> NewHabit {
        NewHabit {
            name: "Lunge-and-reach".to_string(),
            instructions: "5 slow reps/leg, reach overhead".to_string(),
            media_path: None,
            category: Category::Exercise,
            enabled: true,
            trigger_kind: TriggerKind::RotationMember,
            trigger_config_json: "{}".to_string(),
            weight: Some(2),
            rotation_id: None,
            created_at: 1_700_000_000,
        }
    }

    #[test]
    fn inserting_a_habit_returns_an_id_and_it_appears_in_list_habits() {
        // Given a store with no habits yet
        let store = Store::open_in_memory().expect("in-memory store opens");

        // When a new habit is inserted
        let id = store
            .insert_habit(&sample_new_habit())
            .expect("insert succeeds");

        // Then it appears in list_habits with the assigned id and the same content
        let habits = store.list_habits().expect("list succeeds");
        assert_eq!(habits.len(), 1);
        assert_eq!(habits[0].id, id);
        assert_eq!(habits[0].name, "Lunge-and-reach");
        assert_eq!(habits[0].category, Category::Exercise);
        assert_eq!(habits[0].trigger_kind, TriggerKind::RotationMember);
        assert_eq!(habits[0].weight, Some(2));
    }

    #[test]
    fn listing_habits_returns_them_ordered_by_insertion() {
        // Given two habits inserted in a known order
        let store = Store::open_in_memory().expect("in-memory store opens");
        let first = sample_new_habit();
        let second = NewHabit {
            name: "Glute bridges".to_string(),
            weight: Some(1),
            ..sample_new_habit()
        };
        let first_id = store.insert_habit(&first).expect("insert succeeds");
        let second_id = store.insert_habit(&second).expect("insert succeeds");

        // When listing habits
        let habits = store.list_habits().expect("list succeeds");

        // Then they come back in insertion order
        assert_eq!(
            habits.iter().map(|h| h.id).collect::<Vec<_>>(),
            vec![first_id, second_id]
        );
    }

    #[test]
    fn updating_a_habit_changes_its_stored_fields() {
        // Given an inserted habit
        let store = Store::open_in_memory().expect("in-memory store opens");
        let id = store
            .insert_habit(&sample_new_habit())
            .expect("insert succeeds");
        let mut habit = store.list_habits().expect("list succeeds").remove(0);
        assert_eq!(habit.id, id);

        // When it is updated with a new name and category
        habit.name = "Renamed drill".to_string();
        habit.category = Category::General;
        store.update_habit(&habit).expect("update succeeds");

        // Then the stored row reflects the change
        let reloaded = store.list_habits().expect("list succeeds").remove(0);
        assert_eq!(reloaded.name, "Renamed drill");
        assert_eq!(reloaded.category, Category::General);
    }

    #[test]
    fn updating_a_nonexistent_habit_fails_loudly() {
        // Given a store with no habits
        let store = Store::open_in_memory().expect("in-memory store opens");
        let phantom = Habit {
            id: 999,
            name: "Ghost".to_string(),
            instructions: "n/a".to_string(),
            media_path: None,
            category: Category::General,
            enabled: true,
            trigger_kind: TriggerKind::RotationMember,
            trigger_config_json: "{}".to_string(),
            weight: None,
            rotation_id: None,
            created_at: 0,
        };

        // When updating a habit id that was never inserted
        let result = store.update_habit(&phantom);

        // Then the store reports NotFound rather than silently succeeding
        assert!(matches!(result, Err(StoreError::NotFound { id: 999 })));
    }

    #[test]
    fn disabling_a_habit_flips_enabled_to_false_without_deleting_it() {
        // Given an enabled habit
        let store = Store::open_in_memory().expect("in-memory store opens");
        let id = store
            .insert_habit(&sample_new_habit())
            .expect("insert succeeds");

        // When it is disabled
        store.disable_habit(id).expect("disable succeeds");

        // Then it still exists but with enabled = false
        let habits = store.list_habits().expect("list succeeds");
        assert_eq!(habits.len(), 1);
        assert!(!habits[0].enabled);
    }

    #[test]
    fn disabling_a_nonexistent_habit_fails_loudly() {
        // Given a store with no habits
        let store = Store::open_in_memory().expect("in-memory store opens");

        // When disabling an id that was never inserted
        let result = store.disable_habit(42);

        // Then the store reports NotFound rather than silently succeeding
        assert!(matches!(result, Err(StoreError::NotFound { id: 42 })));
    }

    #[test]
    fn the_category_check_constraint_rejects_invalid_values_at_the_db_level() {
        // Given a store (schema created)
        let store = Store::open_in_memory().expect("in-memory store opens");

        // When raw SQL tries to insert a habit with a category outside the enum
        let result = store.conn.execute(
            "INSERT INTO habits (
                name, instructions, media_path, category, enabled,
                trigger_kind, trigger_config_json, weight, rotation_id, created_at
             ) VALUES ('x', 'x', NULL, 'not-a-category', 1, 'rotation-member', '{}', NULL, NULL, 0)",
            [],
        );

        // Then the CHECK constraint rejects it — defence in depth alongside the Rust enum
        assert!(result.is_err());
    }
}
