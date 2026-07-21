use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use rusqlite::{params, Row};

use super::{Store, StoreError};

/// The outcome logged for a habit occurrence (design spec §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventAction {
    Done,
    Skipped,
    Snoozed,
    Expired,
}

impl EventAction {
    fn as_str(self) -> &'static str {
        match self {
            EventAction::Done => "done",
            EventAction::Skipped => "skipped",
            EventAction::Snoozed => "snoozed",
            EventAction::Expired => "expired",
        }
    }
}

impl ToSql for EventAction {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for EventAction {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "done" => Ok(EventAction::Done),
            "skipped" => Ok(EventAction::Skipped),
            "snoozed" => Ok(EventAction::Snoozed),
            "expired" => Ok(EventAction::Expired),
            other => Err(FromSqlError::Other(
                format!("unknown event action: {other}").into(),
            )),
        }
    }
}

/// A logged habit occurrence, as persisted in the store.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub id: i64,
    pub habit_id: i64,
    pub action: EventAction,
    pub at: i64,
}

/// Fields required to append a new event; the store assigns `id`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewEvent {
    pub habit_id: i64,
    pub action: EventAction,
    pub at: i64,
}

fn row_to_event(row: &Row) -> rusqlite::Result<Event> {
    Ok(Event {
        id: row.get(0)?,
        habit_id: row.get(1)?,
        action: row.get(2)?,
        at: row.get(3)?,
    })
}

impl Store {
    /// Appends an event to the log, returning the id SQLite assigned to it.
    /// Events are append-only — the store never updates or deletes them.
    pub fn append_event(&self, event: &NewEvent) -> Result<i64, StoreError> {
        self.conn.execute(
            "INSERT INTO events (habit_id, action, at) VALUES (?1, ?2, ?3)",
            params![event.habit_id, event.action, event.at],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Lists every logged event, ordered chronologically.
    pub fn list_events(&self) -> Result<Vec<Event>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, habit_id, action, at FROM events ORDER BY at, id")?;
        let events = stmt
            .query_map([], row_to_event)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::habits::{Category, NewHabit, TriggerKind};
    use crate::store::Store;

    /// Inserts a habit so events have a valid `habit_id` to reference.
    fn insert_sample_habit(store: &Store) -> i64 {
        store
            .insert_habit(&NewHabit {
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
            })
            .expect("habit insert succeeds")
    }

    #[test]
    fn appending_an_event_returns_an_id_and_it_appears_in_list_events() {
        // Given a store with a habit
        let store = Store::open_in_memory().expect("in-memory store opens");
        let habit_id = insert_sample_habit(&store);

        // When a "done" event is appended for it
        let event_id = store
            .append_event(&NewEvent {
                habit_id,
                action: EventAction::Done,
                at: 1_700_000_100,
            })
            .expect("append succeeds");

        // Then it appears in list_events with the assigned id and the same content
        let events = store.list_events().expect("list succeeds");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, event_id);
        assert_eq!(events[0].habit_id, habit_id);
        assert_eq!(events[0].action, EventAction::Done);
        assert_eq!(events[0].at, 1_700_000_100);
    }

    #[test]
    fn events_are_listed_in_chronological_order() {
        // Given two events appended out of chronological order
        let store = Store::open_in_memory().expect("in-memory store opens");
        let habit_id = insert_sample_habit(&store);
        store
            .append_event(&NewEvent {
                habit_id,
                action: EventAction::Skipped,
                at: 200,
            })
            .expect("append succeeds");
        store
            .append_event(&NewEvent {
                habit_id,
                action: EventAction::Done,
                at: 100,
            })
            .expect("append succeeds");

        // When listing events
        let events = store.list_events().expect("list succeeds");

        // Then they come back ordered by `at`, not insertion order
        assert_eq!(
            events.iter().map(|e| e.at).collect::<Vec<_>>(),
            vec![100, 200]
        );
    }

    #[test]
    fn appending_an_event_for_a_nonexistent_habit_fails_loudly() {
        // Given a store with no habits at all
        let store = Store::open_in_memory().expect("in-memory store opens");

        // When appending an event that references a habit id that doesn't exist
        let result = store.append_event(&NewEvent {
            habit_id: 999,
            action: EventAction::Done,
            at: 0,
        });

        // Then the foreign-key constraint rejects it rather than logging orphaned data
        assert!(result.is_err());
    }
}
