use rusqlite::Connection;

use super::StoreError;

/// Creates every table the store needs, if it does not already exist.
/// Idempotent — safe to call on every `Store::open` (see design spec §5).
pub(super) fn migrate(conn: &Connection) -> Result<(), StoreError> {
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
            at INTEGER NOT NULL
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
            start_at_login INTEGER NOT NULL
        );
        ",
    )?;
    Ok(())
}
