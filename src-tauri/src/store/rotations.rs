use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use rusqlite::{params, Row};

use super::{Store, StoreError};

/// How a rotation's active window is determined (design spec §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowKind {
    /// The rotation has its own start/end, independent of the global window.
    Own,
    /// The rotation borrows the global day window from config.
    InheritGlobal,
    /// The rotation has no window — it is active around the clock.
    AlwaysOn,
}

impl WindowKind {
    fn as_str(self) -> &'static str {
        match self {
            WindowKind::Own => "own",
            WindowKind::InheritGlobal => "inherit-global",
            WindowKind::AlwaysOn => "always-on",
        }
    }
}

impl ToSql for WindowKind {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for WindowKind {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "own" => Ok(WindowKind::Own),
            "inherit-global" => Ok(WindowKind::InheritGlobal),
            "always-on" => Ok(WindowKind::AlwaysOn),
            other => Err(FromSqlError::Other(
                format!("unknown window kind: {other}").into(),
            )),
        }
    }
}

/// A rotation as persisted in the store: interval + window; members are the
/// habits whose `rotation_id` points back at it.
#[derive(Debug, Clone, PartialEq)]
pub struct Rotation {
    pub id: i64,
    pub name: String,
    pub interval_secs: i64,
    pub window_kind: WindowKind,
    pub window_start: Option<String>,
    pub window_end: Option<String>,
}

/// Fields required to insert a new rotation; the store assigns `id`.
#[derive(Debug, Clone, PartialEq)]
pub struct NewRotation {
    pub name: String,
    pub interval_secs: i64,
    pub window_kind: WindowKind,
    pub window_start: Option<String>,
    pub window_end: Option<String>,
}

fn row_to_rotation(row: &Row) -> rusqlite::Result<Rotation> {
    Ok(Rotation {
        id: row.get(0)?,
        name: row.get(1)?,
        interval_secs: row.get(2)?,
        window_kind: row.get(3)?,
        window_start: row.get(4)?,
        window_end: row.get(5)?,
    })
}

impl Store {
    /// Inserts a new rotation, returning the id SQLite assigned to it.
    pub fn insert_rotation(&self, rotation: &NewRotation) -> Result<i64, StoreError> {
        self.conn.execute(
            "INSERT INTO rotations (name, interval_secs, window_kind, window_start, window_end)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                rotation.name,
                rotation.interval_secs,
                rotation.window_kind,
                rotation.window_start,
                rotation.window_end,
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Lists every rotation, ordered by insertion.
    pub fn list_rotations(&self) -> Result<Vec<Rotation>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, interval_secs, window_kind, window_start, window_end
             FROM rotations ORDER BY id",
        )?;
        let rotations = stmt
            .query_map([], row_to_rotation)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rotations)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    /// The seed rotation from the design spec (§7): every 30 minutes,
    /// inheriting the global day window.
    fn sample_new_rotation() -> NewRotation {
        NewRotation {
            name: "Movement snacks".to_string(),
            interval_secs: 1_800,
            window_kind: WindowKind::InheritGlobal,
            window_start: None,
            window_end: None,
        }
    }

    #[test]
    fn inserting_a_rotation_returns_an_id_and_it_appears_in_list_rotations() {
        // Given a store with no rotations yet
        let store = Store::open_in_memory().expect("in-memory store opens");

        // When a new rotation is inserted
        let id = store
            .insert_rotation(&sample_new_rotation())
            .expect("insert succeeds");

        // Then it appears in list_rotations with the assigned id and the same content
        let rotations = store.list_rotations().expect("list succeeds");
        assert_eq!(rotations.len(), 1);
        assert_eq!(rotations[0].id, id);
        assert_eq!(rotations[0].interval_secs, 1_800);
        assert_eq!(rotations[0].window_kind, WindowKind::InheritGlobal);
    }

    #[test]
    fn own_window_rotations_round_trip_their_start_and_end() {
        // Given a rotation with its own custom window
        let store = Store::open_in_memory().expect("in-memory store opens");
        let rotation = NewRotation {
            window_kind: WindowKind::Own,
            window_start: Some("07:00".to_string()),
            window_end: Some("08:30".to_string()),
            ..sample_new_rotation()
        };

        // When it is inserted and read back
        store.insert_rotation(&rotation).expect("insert succeeds");
        let stored = store.list_rotations().expect("list succeeds").remove(0);

        // Then its window bounds are preserved exactly
        assert_eq!(stored.window_start, Some("07:00".to_string()));
        assert_eq!(stored.window_end, Some("08:30".to_string()));
    }

    #[test]
    fn inherit_global_and_always_on_rotations_have_no_window_bounds() {
        // Given rotations that don't own a window
        let store = Store::open_in_memory().expect("in-memory store opens");
        store
            .insert_rotation(&sample_new_rotation())
            .expect("insert succeeds");
        store
            .insert_rotation(&NewRotation {
                name: "Always-on nudges".to_string(),
                window_kind: WindowKind::AlwaysOn,
                ..sample_new_rotation()
            })
            .expect("insert succeeds");

        // When read back
        let rotations = store.list_rotations().expect("list succeeds");

        // Then neither has a start/end — they aren't `own`
        for rotation in &rotations {
            assert_eq!(rotation.window_start, None);
            assert_eq!(rotation.window_end, None);
        }
    }
}
