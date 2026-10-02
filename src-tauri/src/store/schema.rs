use rusqlite::{Connection, Transaction, TransactionBehavior};

use super::StoreError;
use crate::clock::Zone;

/// Creates every table the store needs, if it does not already exist, then
/// applies any additive column migrations (see design spec §5).
/// Then runs the `PRAGMA user_version` data migrations, interpreting legacy
/// wall-clock values in `zone`. Idempotent — safe to call on every
/// `Store::open`.
///
/// Everything runs in one `IMMEDIATE` transaction: the app and its headless
/// MCP server open the same file, so two connections can migrate at once, and
/// taking the write lock up front makes the second wait and then see the
/// first's finished schema and `user_version` instead of re-applying them.
pub(super) fn migrate(conn: &Connection, zone: Zone) -> Result<(), StoreError> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    create_tables(&tx)?;
    add_events_shown_at_column(&tx)?;
    add_config_mic_pause_enabled_column(&tx)?;
    migrate_event_times_to_utc(&tx, zone)?;
    tx.commit()?;
    Ok(())
}

/// `PRAGMA user_version` once event times are true UTC epoch seconds.
const UTC_EVENT_TIMES_VERSION: i64 = 1;

/// Version 0 -> 1: rewrites `events.at` and non-null `events.shown_at` from
/// the legacy "naive-local-as-UTC" encoding (a local wall-clock time stored as
/// if it were UTC) to true UTC epoch seconds, resolving each wall-clock time
/// in `zone`. The rewrite and the version bump share `migrate`'s transaction, so
/// a failure leaves the database at version 0 to be retried. A database already
/// at version 1 or later is left untouched; a fresh one has no events, so the
/// rewrite is a no-op and it is simply stamped. `habits.created_at` was
/// always true UTC and `config`/`rotations` hold wall-clock strings, so
/// neither is touched.
fn migrate_event_times_to_utc(conn: &Connection, zone: Zone) -> Result<(), StoreError> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version >= UTC_EVENT_TIMES_VERSION {
        return Ok(());
    }

    let rows: Vec<(i64, i64, Option<i64>)> = {
        let mut stmt = conn.prepare("SELECT id, at, shown_at FROM events")?;
        let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect::<Result<_, _>>()?
    };
    for (id, at, shown_at) in rows {
        conn.execute(
            "UPDATE events SET at = ?1, shown_at = ?2 WHERE id = ?3",
            rusqlite::params![
                legacy_to_utc(at, zone),
                shown_at.map(|shown| legacy_to_utc(shown, zone)),
                id
            ],
        )?;
    }
    conn.pragma_update(None, "user_version", UTC_EVENT_TIMES_VERSION)?;
    Ok(())
}

/// Converts one legacy naive-local-as-UTC epoch to a true UTC epoch. An epoch
/// outside chrono's range is left as is rather than aborting the migration.
fn legacy_to_utc(legacy: i64, zone: Zone) -> i64 {
    match chrono::DateTime::from_timestamp(legacy, 0) {
        Some(naive_as_utc) => zone.resolve(naive_as_utc.naive_utc()).timestamp(),
        None => legacy,
    }
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
    use std::sync::{Arc, Barrier};
    use std::thread;

    use crate::clock::Zone;
    use chrono_tz::Tz;

    /// The zone the pre-existing column-migration tests run under; they never
    /// carry events, so the choice is immaterial to them.
    const ZONE: Zone = Zone::Named(Tz::Europe__London);

    fn user_version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("pragma query succeeds")
    }

    /// Builds a database as the app wrote it before the UTC migration: current
    /// tables, `user_version` 0, and one habit to hang events off.
    fn legacy_database_with_habit() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory connection opens");
        seed_legacy_database_with_habit(&conn);
        conn
    }

    fn seed_legacy_database_with_habit(conn: &Connection) {
        migrate(conn, ZONE).expect("initial migration succeeds");
        conn.execute_batch(
            "PRAGMA user_version = 0;
             INSERT INTO habits (
                 name, instructions, category, enabled, trigger_kind,
                 trigger_config_json, created_at
             ) VALUES ('Stretch', '', 'general', 1, 'rotation-member', '{}', 0);",
        )
        .expect("legacy habit inserts");
    }

    fn insert_event(conn: &Connection, at: i64, shown_at: Option<i64>) {
        conn.execute(
            "INSERT INTO events (habit_id, action, at, shown_at) VALUES (1, 'done', ?1, ?2)",
            rusqlite::params![at, shown_at],
        )
        .expect("event inserts");
    }

    fn event_times(conn: &Connection) -> Vec<(i64, Option<i64>)> {
        let mut stmt = conn
            .prepare("SELECT at, shown_at FROM events ORDER BY id")
            .expect("prepare succeeds");
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query succeeds")
            .collect::<Result<_, _>>()
            .expect("rows are readable")
    }

    /// Epoch seconds of a wall-clock time read as if it were UTC — the legacy
    /// "naive-local-as-UTC" encoding.
    fn naive_local_as_utc(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        chrono::NaiveDate::from_ymd_opt(y, mo, d)
            .and_then(|date| date.and_hms_opt(h, mi, 0))
            .expect("valid date")
            .and_utc()
            .timestamp()
    }

    #[test]
    fn migrating_a_legacy_summer_event_in_vienna_shifts_it_back_by_the_offset() {
        // Given an event stored as 10:00 local wall-clock read as UTC, from a
        // Vienna install in CEST (UTC+2)
        let conn = legacy_database_with_habit();
        let legacy = naive_local_as_utc(2026, 7, 1, 10, 0);
        insert_event(&conn, legacy, Some(legacy - 60));

        // When the data migration runs in Europe/Vienna
        migrate(&conn, Zone::Named(Tz::Europe__Vienna)).expect("migration succeeds");

        // Then `at` and `shown_at` are true UTC, two hours earlier
        assert_eq!(
            event_times(&conn),
            vec![(legacy - 7200, Some(legacy - 60 - 7200))]
        );
        assert_eq!(user_version(&conn), 1);
    }

    #[test]
    fn migrating_a_legacy_winter_event_in_london_leaves_it_unchanged() {
        // Given an event stored as 10:00 local wall-clock read as UTC, from a
        // London install in GMT (UTC+0)
        let conn = legacy_database_with_habit();
        let legacy = naive_local_as_utc(2026, 1, 15, 10, 0);
        insert_event(&conn, legacy, None);

        // When the data migration runs in Europe/London
        migrate(&conn, ZONE).expect("migration succeeds");

        // Then the epoch is already true UTC
        assert_eq!(event_times(&conn), vec![(legacy, None)]);
    }

    #[test]
    fn migrating_keeps_a_null_shown_at_null() {
        // Given a legacy event that was never shown (no `shown_at`)
        let conn = legacy_database_with_habit();
        insert_event(&conn, naive_local_as_utc(2026, 7, 1, 10, 0), None);

        // When the data migration runs
        migrate(&conn, Zone::Named(Tz::Europe__Vienna)).expect("migration succeeds");

        // Then `shown_at` is still NULL
        assert_eq!(event_times(&conn)[0].1, None);
    }

    #[test]
    fn re_running_the_utc_migration_does_not_shift_events_again() {
        // Given a legacy database already migrated once
        let conn = legacy_database_with_habit();
        let legacy = naive_local_as_utc(2026, 7, 1, 10, 0);
        insert_event(&conn, legacy, Some(legacy));
        let vienna = Zone::Named(Tz::Europe__Vienna);
        migrate(&conn, vienna).expect("first migration succeeds");
        let after_first = event_times(&conn);

        // When the migration runs again
        migrate(&conn, vienna).expect("second migration succeeds");

        // Then the events are untouched
        assert_eq!(event_times(&conn), after_first);
        assert_eq!(user_version(&conn), 1);
    }

    #[test]
    fn migrating_a_fresh_database_records_the_current_version() {
        // Given a brand-new database
        let conn = Connection::open_in_memory().expect("in-memory connection opens");
        assert_eq!(user_version(&conn), 0);

        // When migrating
        migrate(&conn, ZONE).expect("migration succeeds");

        // Then it is stamped as already on the UTC encoding, with no events
        assert_eq!(user_version(&conn), 1);
        assert!(event_times(&conn).is_empty());
    }

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
        migrate(&conn, ZONE).expect("migration succeeds");

        // Then the column is added
        assert!(events_has_shown_at_column(&conn));
    }

    #[test]
    fn re_running_the_shown_at_migration_on_an_upgraded_database_is_a_no_op() {
        // Given a pre-`shown_at` database already upgraded once
        let conn = connection_with_pre_shown_at_schema();
        migrate(&conn, ZONE).expect("first migration succeeds");
        assert!(events_has_shown_at_column(&conn));

        // When the migration runs again
        let result = migrate(&conn, ZONE);

        // Then it succeeds without error, and the column is unchanged
        assert!(result.is_ok());
        assert!(events_has_shown_at_column(&conn));
    }

    #[test]
    fn migrating_a_fresh_database_already_has_the_shown_at_column() {
        // Given a brand-new database, migrated once (as `Store::open` does)
        let conn = Connection::open_in_memory().expect("in-memory connection opens");

        // When migrating
        migrate(&conn, ZONE).expect("migration succeeds");

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
        migrate(&conn, ZONE).expect("migration succeeds");

        // Then the column is added, and the existing row adopts the enabled
        // default so upgraded installs get the mic quiet rule switched on
        assert!(config_has_mic_pause_column(&conn));
        let enabled: i64 = conn
            .query_row(
                "SELECT mic_pause_enabled FROM config WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .expect("row present");
        assert_eq!(enabled, 1);
    }

    #[test]
    fn re_running_the_mic_pause_migration_on_an_upgraded_database_is_a_no_op() {
        // Given a pre-`mic_pause_enabled` database already upgraded once
        let conn = connection_with_pre_mic_pause_config_schema();
        migrate(&conn, ZONE).expect("first migration succeeds");
        assert!(config_has_mic_pause_column(&conn));

        // When the migration runs again
        let result = migrate(&conn, ZONE);

        // Then it succeeds without error, and the column is unchanged
        assert!(result.is_ok());
        assert!(config_has_mic_pause_column(&conn));
    }

    #[test]
    fn migrating_a_fresh_database_already_has_the_mic_pause_column() {
        // Given a brand-new database, migrated once (as `Store::open` does)
        let conn = Connection::open_in_memory().expect("in-memory connection opens");

        // When migrating
        migrate(&conn, ZONE).expect("migration succeeds");

        // Then the fresh `config` table already carries `mic_pause_enabled` —
        // the guard is a no-op for databases that never lacked it
        assert!(config_has_mic_pause_column(&conn));
    }

    #[test]
    fn two_processes_migrating_the_same_legacy_file_at_once_shift_events_only_once() {
        // The app and its headless MCP server open the same file, so both can
        // run the migration at the same moment. Repeated to make an
        // unserialised interleaving likely to show up.
        for _ in 0..20 {
            // Given a legacy file-backed database with one CEST event
            let dir = tempfile::tempdir().expect("temp dir");
            let path = dir.path().join("budgebell.sqlite");
            let legacy = naive_local_as_utc(2026, 7, 1, 10, 0);
            {
                let conn = Connection::open(&path).expect("file opens");
                seed_legacy_database_with_habit(&conn);
                insert_event(&conn, legacy, None);
            }

            // When two connections migrate it concurrently, each waiting on
            // the other's lock as `Store::open` connections do
            let barrier = Arc::new(Barrier::new(2));
            let handles: Vec<_> = (0..2)
                .map(|_| {
                    let path = path.clone();
                    let barrier = Arc::clone(&barrier);
                    thread::spawn(move || {
                        let conn = Connection::open(&path).expect("file opens");
                        conn.busy_timeout(super::super::BUSY_TIMEOUT)
                            .expect("busy timeout sets");
                        barrier.wait();
                        migrate(&conn, Zone::Named(Tz::Europe__Vienna))
                    })
                })
                .collect();
            for handle in handles {
                handle
                    .join()
                    .expect("migration thread completes")
                    .expect("migration succeeds");
            }

            // Then the event moved back by the CEST offset exactly once
            let conn = Connection::open(&path).expect("file opens");
            assert_eq!(event_times(&conn), vec![(legacy - 7200, None)]);
        }
    }
}
