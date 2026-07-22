use rusqlite::Connection;

use super::StoreError;

/// Creates every table the store needs, if it does not already exist, then
/// applies any additive column migrations (see design spec §5).
/// Idempotent — safe to call on every `Store::open`.
pub(super) fn migrate(conn: &Connection) -> Result<(), StoreError> {
    create_tables(conn)?;
    add_events_shown_at_column(conn)?;
    add_config_mic_pause_enabled_column(conn)?;
    Ok(())
}

fn create_tables(conn: &Connection) -> Result<(), StoreError> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS rotations (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            interval_secs INTEGER NOT NULL,
            window_kind TEXT NOT NULL
                CHECK (window_kind IN ('own', 'inherit-global', 'always-on')),
            window_start TEXT,
            window_end TEXT
        );

        CREATE TABLE IF NOT EXISTS habits (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            instructions TEXT NOT NULL,
            media_path TEXT,
            category TEXT NOT NULL
                CHECK (category IN ('exercise', 'general')),
            enabled INTEGER NOT NULL,
            trigger_kind TEXT NOT NULL
                CHECK (trigger_kind IN (
                    'rotation-member', 'schedule-at-time', 'schedule-weekly-count'
                )),
            trigger_config_json TEXT NOT NULL,
            weight INTEGER,
            rotation_id INTEGER REFERENCES rotations(id),
            created_at INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            habit_id INTEGER NOT NULL REFERENCES habits(id),
            action TEXT NOT NULL
                CHECK (action IN ('done', 'skipped', 'snoozed', 'expired')),
            at INTEGER NOT NULL,
            shown_at INTEGER
        );

        CREATE TABLE IF NOT EXISTS config (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            day_rollover TEXT NOT NULL,
            day_window_start TEXT NOT NULL,
            day_window_end TEXT NOT NULL,
            calendar_pause_enabled INTEGER NOT NULL,
            calendar_mode TEXT NOT NULL
                CHECK (calendar_mode IN ('all', 'with-others')),
            idle_enabled INTEGER NOT NULL,
            dnd_enabled INTEGER NOT NULL,
            mic_pause_enabled INTEGER NOT NULL DEFAULT 1,
            start_at_login INTEGER NOT NULL
        );
        ",
    )?;
    Ok(())
}

/// Adds `events.shown_at` to a database created before it existed (design
/// spec §5). Guarded by inspecting `PRAGMA table_info(events)` so a database
/// that already has the column — including one just created above — is left
/// untouched; re-running this is always a no-op.
fn add_events_shown_at_column(conn: &Connection) -> Result<(), StoreError> {
    let has_shown_at: bool = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('events') WHERE name = 'shown_at'",
        [],
        |row| row.get::<_, i64>(0),
    )? > 0;
    if !has_shown_at {
        conn.execute_batch("ALTER TABLE events ADD COLUMN shown_at INTEGER")?;
    }
    Ok(())
}

/// Adds `config.mic_pause_enabled` to a database created before the microphone
/// quiet rule shipped (design spec §4.5/§5). Guarded by inspecting
/// `PRAGMA table_info(config)` so a database that already has the column —
/// including one just created above — is left untouched; re-running this is
/// always a no-op. The column defaults to `1` (enabled) so existing installs
/// adopt the new quiet rule on upgrade, matching the seed default.
fn add_config_mic_pause_enabled_column(conn: &Connection) -> Result<(), StoreError> {
    let has_mic_pause: bool = conn.query_row(
        "SELECT COUNT(*) FROM pragma_table_info('config') WHERE name = 'mic_pause_enabled'",
        [],
        |row| row.get::<_, i64>(0),
    )? > 0;
    if !has_mic_pause {
        conn.execute_batch(
            "ALTER TABLE config ADD COLUMN mic_pause_enabled INTEGER NOT NULL DEFAULT 1",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Creates a connection with the pre-`shown_at` `events` schema, as an
    /// existing database on disk would have before this migration shipped.
    fn connection_with_pre_shown_at_schema() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory connection opens");
        conn.execute_batch(
            "
            CREATE TABLE habits (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                instructions TEXT NOT NULL,
                media_path TEXT,
                category TEXT NOT NULL,
                enabled INTEGER NOT NULL,
                trigger_kind TEXT NOT NULL,
                trigger_config_json TEXT NOT NULL,
                weight INTEGER,
                rotation_id INTEGER,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                habit_id INTEGER NOT NULL REFERENCES habits(id),
                action TEXT NOT NULL,
                at INTEGER NOT NULL
            );
            ",
        )
        .expect("pre-shown_at schema creates");
        conn
    }

    fn events_has_shown_at_column(conn: &Connection) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('events') WHERE name = 'shown_at'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("pragma query succeeds")
            > 0
    }

    #[test]
    fn migrating_a_pre_shown_at_database_adds_the_column() {
        // Given a database created before `events.shown_at` existed
        let conn = connection_with_pre_shown_at_schema();
        assert!(!events_has_shown_at_column(&conn));

        // When the migration runs
        migrate(&conn).expect("migration succeeds");

        // Then the column is added
        assert!(events_has_shown_at_column(&conn));
    }

    #[test]
    fn re_running_the_shown_at_migration_on_an_upgraded_database_is_a_no_op() {
        // Given a pre-`shown_at` database already upgraded once
        let conn = connection_with_pre_shown_at_schema();
        migrate(&conn).expect("first migration succeeds");
        assert!(events_has_shown_at_column(&conn));

        // When the migration runs again
        let result = migrate(&conn);

        // Then it succeeds without error, and the column is unchanged
        assert!(result.is_ok());
        assert!(events_has_shown_at_column(&conn));
    }

    #[test]
    fn migrating_a_fresh_database_already_has_the_shown_at_column() {
        // Given a brand-new database, migrated once (as `Store::open` does)
        let conn = Connection::open_in_memory().expect("in-memory connection opens");

        // When migrating
        migrate(&conn).expect("migration succeeds");

        // Then the fresh `events` table already carries `shown_at` — the
        // guard is a no-op for databases that never lacked it
        assert!(events_has_shown_at_column(&conn));
    }

    /// Creates a connection with the pre-`mic_pause_enabled` `config` schema
    /// and a single seeded row, as an existing database on disk would have
    /// before the microphone quiet rule shipped.
    fn connection_with_pre_mic_pause_config_schema() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory connection opens");
        conn.execute_batch(
            "
            CREATE TABLE config (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                day_rollover TEXT NOT NULL,
                day_window_start TEXT NOT NULL,
                day_window_end TEXT NOT NULL,
                calendar_pause_enabled INTEGER NOT NULL,
                calendar_mode TEXT NOT NULL,
                idle_enabled INTEGER NOT NULL,
                dnd_enabled INTEGER NOT NULL,
                start_at_login INTEGER NOT NULL
            );

            INSERT INTO config (
                id, day_rollover, day_window_start, day_window_end,
                calendar_pause_enabled, calendar_mode, idle_enabled,
                dnd_enabled, start_at_login
            ) VALUES (1, '04:00', '09:00', '18:00', 1, 'with-others', 1, 1, 0);
            ",
        )
        .expect("pre-mic-pause config schema creates");
        conn
    }

    fn config_has_mic_pause_column(conn: &Connection) -> bool {
        conn.query_row(
            "SELECT COUNT(*) FROM pragma_table_info('config') WHERE name = 'mic_pause_enabled'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .expect("pragma query succeeds")
            > 0
    }

    #[test]
    fn migrating_a_pre_mic_pause_database_adds_the_column_defaulting_to_enabled() {
        // Given a database created before `config.mic_pause_enabled` existed,
        // carrying an existing settings row
        let conn = connection_with_pre_mic_pause_config_schema();
        assert!(!config_has_mic_pause_column(&conn));

        // When the migration runs
        migrate(&conn).expect("migration succeeds");

        // Then the column is added, and the existing row adopts the enabled
        // default so upgraded installs get the mic quiet rule switched on
        assert!(config_has_mic_pause_column(&conn));
        let enabled: i64 = conn
            .query_row("SELECT mic_pause_enabled FROM config WHERE id = 1", [], |row| {
                row.get(0)
            })
            .expect("row present");
        assert_eq!(enabled, 1);
    }

    #[test]
    fn re_running_the_mic_pause_migration_on_an_upgraded_database_is_a_no_op() {
        // Given a pre-`mic_pause_enabled` database already upgraded once
        let conn = connection_with_pre_mic_pause_config_schema();
        migrate(&conn).expect("first migration succeeds");
        assert!(config_has_mic_pause_column(&conn));

        // When the migration runs again
        let result = migrate(&conn);

        // Then it succeeds without error, and the column is unchanged
        assert!(result.is_ok());
        assert!(config_has_mic_pause_column(&conn));
    }

    #[test]
    fn migrating_a_fresh_database_already_has_the_mic_pause_column() {
        // Given a brand-new database, migrated once (as `Store::open` does)
        let conn = Connection::open_in_memory().expect("in-memory connection opens");

        // When migrating
        migrate(&conn).expect("migration succeeds");

        // Then the fresh `config` table already carries `mic_pause_enabled` —
        // the guard is a no-op for databases that never lacked it
        assert!(config_has_mic_pause_column(&conn));
    }
}
