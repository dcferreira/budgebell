use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use rusqlite::{params, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use super::{Store, StoreError};

/// Which calendar events count as a "real meeting" for the calendar pause
/// quiet rule (design spec §3.6 / §4.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CalendarMode {
    All,
    WithOthers,
}

impl CalendarMode {
    fn as_str(self) -> &'static str {
        match self {
            CalendarMode::All => "all",
            CalendarMode::WithOthers => "with-others",
        }
    }
}

impl ToSql for CalendarMode {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for CalendarMode {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "all" => Ok(CalendarMode::All),
            "with-others" => Ok(CalendarMode::WithOthers),
            other => Err(FromSqlError::Other(
                format!("unknown calendar mode: {other}").into(),
            )),
        }
    }
}

/// The single-row app-wide settings (design spec §5 / §3.6). There is at
/// most one row, keyed at id 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub day_rollover: String,
    pub day_window_start: String,
    pub day_window_end: String,
    pub calendar_pause_enabled: bool,
    pub calendar_mode: CalendarMode,
    pub idle_enabled: bool,
    pub dnd_enabled: bool,
    pub mic_pause_enabled: bool,
    pub start_at_login: bool,
}

fn row_to_config(row: &Row) -> rusqlite::Result<Config> {
    Ok(Config {
        day_rollover: row.get(0)?,
        day_window_start: row.get(1)?,
        day_window_end: row.get(2)?,
        calendar_pause_enabled: row.get(3)?,
        calendar_mode: row.get(4)?,
        idle_enabled: row.get(5)?,
        dnd_enabled: row.get(6)?,
        mic_pause_enabled: row.get(7)?,
        start_at_login: row.get(8)?,
    })
}

impl Store {
    /// Reads the app config, or `None` if it has never been written —
    /// seeding the defaults is the caller's responsibility (see the
    /// `seed-wire` task in the design spec §10).
    pub fn read_config(&self) -> Result<Option<Config>, StoreError> {
        let config = self
            .conn
            .query_row(
                "SELECT day_rollover, day_window_start, day_window_end,
                        calendar_pause_enabled, calendar_mode, idle_enabled,
                        dnd_enabled, mic_pause_enabled, start_at_login
                 FROM config WHERE id = 1",
                [],
                row_to_config,
            )
            .optional()?;
        Ok(config)
    }

    /// Writes the app config, replacing any previously stored values.
    pub fn write_config(&self, config: &Config) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO config (
                id, day_rollover, day_window_start, day_window_end,
                calendar_pause_enabled, calendar_mode, idle_enabled,
                dnd_enabled, mic_pause_enabled, start_at_login
             ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                day_rollover = excluded.day_rollover,
                day_window_start = excluded.day_window_start,
                day_window_end = excluded.day_window_end,
                calendar_pause_enabled = excluded.calendar_pause_enabled,
                calendar_mode = excluded.calendar_mode,
                idle_enabled = excluded.idle_enabled,
                dnd_enabled = excluded.dnd_enabled,
                mic_pause_enabled = excluded.mic_pause_enabled,
                start_at_login = excluded.start_at_login",
            params![
                config.day_rollover,
                config.day_window_start,
                config.day_window_end,
                config.calendar_pause_enabled,
                config.calendar_mode,
                config.idle_enabled,
                config.dnd_enabled,
                config.mic_pause_enabled,
                config.start_at_login,
            ],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    /// The default config from the design spec §7: rollover 04:00, day
    /// window 09:00–18:00, calendar mode "with-others".
    fn sample_config() -> Config {
        Config {
            day_rollover: "04:00".to_string(),
            day_window_start: "09:00".to_string(),
            day_window_end: "18:00".to_string(),
            calendar_pause_enabled: true,
            calendar_mode: CalendarMode::WithOthers,
            idle_enabled: true,
            dnd_enabled: true,
            mic_pause_enabled: true,
            start_at_login: false,
        }
    }

    #[test]
    fn reading_config_before_it_is_written_returns_none() {
        // Given a fresh store where config has never been written
        let store = Store::open_in_memory().expect("in-memory store opens");

        // When reading config
        let config = store.read_config().expect("read succeeds");

        // Then there is no row yet — seeding defaults is the caller's job
        assert_eq!(config, None);
    }

    #[test]
    fn writing_then_reading_config_round_trips_all_fields() {
        // Given a store with no config yet
        let store = Store::open_in_memory().expect("in-memory store opens");

        // When the default config is written
        store
            .write_config(&sample_config())
            .expect("write succeeds");

        // Then reading it back returns exactly what was written
        let config = store.read_config().expect("read succeeds");
        assert_eq!(config, Some(sample_config()));
    }

    #[test]
    fn writing_config_twice_replaces_the_previous_values() {
        // Given a store with an already-written config
        let store = Store::open_in_memory().expect("in-memory store opens");
        store
            .write_config(&sample_config())
            .expect("write succeeds");

        // When it is written again with different values
        let updated = Config {
            day_rollover: "05:00".to_string(),
            calendar_mode: CalendarMode::All,
            start_at_login: true,
            ..sample_config()
        };
        store.write_config(&updated).expect("write succeeds");

        // Then reading it back returns the new values, not the old ones
        let config = store.read_config().expect("read succeeds");
        assert_eq!(config, Some(updated));
    }

    #[test]
    fn the_mic_pause_toggle_round_trips_through_the_store() {
        // Given a store with the microphone quiet rule switched off
        let store = Store::open_in_memory().expect("in-memory store opens");
        let config = Config {
            mic_pause_enabled: false,
            ..sample_config()
        };

        // When it is written and read back
        store.write_config(&config).expect("write succeeds");

        // Then the disabled mic-pause toggle survives the round trip rather
        // than reverting to the enabled default
        let read = store
            .read_config()
            .expect("read succeeds")
            .expect("config row present");
        assert!(!read.mic_pause_enabled);
    }
}
