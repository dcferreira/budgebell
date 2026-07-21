//! The SQLite persistence layer (design spec §5): schema + idempotent
//! migrations, plus typed CRUD for habits, rotations, events, and config.
//! This module owns all SQL; every other layer talks to it only through
//! `Store`'s public methods.

pub mod config;
pub mod error;
pub mod events;
pub mod habits;
pub mod rotations;
mod schema;

use std::path::Path;

use rusqlite::Connection;

pub use config::{CalendarMode, Config};
pub use error::StoreError;
pub use events::{Event, EventAction, NewEvent};
pub use habits::{Category, Habit, NewHabit, TriggerKind};
pub use rotations::{NewRotation, Rotation, WindowKind};

/// A thin wrapper around a SQLite connection providing typed CRUD for the
/// habits domain. Migrations run automatically on open, so the schema is
/// always current before any query executes.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens (creating if necessary) a SQLite database file at `path`.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StoreError> {
        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    /// Opens an in-memory database — used by tests so each test gets an
    /// isolated, disposable store.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let conn = Connection::open_in_memory()?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> Result<Self, StoreError> {
        // Enforced at the connection level: SQLite defaults foreign keys to
        // off, but the schema relies on them (e.g. events -> habits).
        conn.pragma_update(None, "foreign_keys", true)?;
        schema::migrate(&conn)?;
        Ok(Self { conn })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_an_in_memory_store_creates_every_table() {
        // Given a fresh in-memory store
        let store = Store::open_in_memory().expect("in-memory store opens");

        // When querying sqlite_master for the tables the schema defines
        let mut stmt = store
            .conn
            .prepare(
                "SELECT name FROM sqlite_master
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%'
                 ORDER BY name",
            )
            .expect("prepare succeeds");
        let names: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .expect("query succeeds")
            .collect::<Result<_, _>>()
            .expect("rows are readable");

        // Then all four tables from the design spec exist
        assert_eq!(names, vec!["config", "events", "habits", "rotations"]);
    }

    #[test]
    fn opening_the_same_store_twice_is_an_idempotent_migration() {
        // Given a store that has already been opened (and migrated) once
        let store = Store::open_in_memory().expect("in-memory store opens");

        // When migrating the same connection again
        let result = schema::migrate(&store.conn);

        // Then it succeeds without error — `CREATE TABLE IF NOT EXISTS` is idempotent
        assert!(result.is_ok());
    }

    #[test]
    fn a_file_backed_store_persists_data_across_reopens() {
        // Given a habit inserted into a file-backed store
        let path = std::env::temp_dir().join(format!(
            "habits_store_test_{}_{}.sqlite",
            std::process::id(),
            "persists_across_reopens"
        ));
        let _ = std::fs::remove_file(&path);
        {
            let store = Store::open(&path).expect("file-backed store opens");
            store
                .insert_habit(&NewHabit {
                    name: "Glute bridges".to_string(),
                    instructions: "20, or single-leg 10/side".to_string(),
                    media_path: None,
                    category: Category::Exercise,
                    enabled: true,
                    trigger_kind: TriggerKind::RotationMember,
                    trigger_config_json: "{}".to_string(),
                    weight: Some(1),
                    rotation_id: None,
                    created_at: 1_700_000_000,
                })
                .expect("insert succeeds");
        }

        // When the same file is reopened as a new Store
        let reopened = Store::open(&path).expect("file-backed store reopens");
        let habits = reopened.list_habits().expect("list succeeds");

        // Then the previously inserted habit is still there
        assert_eq!(habits.len(), 1);
        assert_eq!(habits[0].name, "Glute bridges");

        let _ = std::fs::remove_file(&path);
    }
}
