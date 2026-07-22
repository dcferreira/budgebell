use std::str::FromStr;

use crate::store;

use super::error::DomainError;
use super::time::{TimeOfDay, TimeWindow};

/// The global scheduling settings (design spec §4.4/§5): the day rollover
/// instant and the default day window a rotation borrows when it inherits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayConfig {
    pub rollover: TimeOfDay,
    pub day_window: TimeWindow,
}

impl TryFrom<&store::Config> for DayConfig {
    type Error = DomainError;

    /// Parses the store's `"HH:MM"` config strings into a validated `DayConfig`.
    fn try_from(config: &store::Config) -> Result<Self, Self::Error> {
        let rollover = TimeOfDay::from_str(&config.day_rollover)?;
        let start = TimeOfDay::from_str(&config.day_window_start)?;
        let end = TimeOfDay::from_str(&config.day_window_end)?;
        Ok(Self {
            rollover,
            day_window: TimeWindow::new(start, end)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::CalendarMode;

    /// The default config from the design spec §7.
    fn sample_store_config() -> store::Config {
        store::Config {
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
    fn the_default_store_config_parses_into_the_default_day_config() {
        // Given the design spec's default config row (§7)
        let store_config = sample_store_config();

        // When converting it into a domain DayConfig
        let day_config = DayConfig::try_from(&store_config).expect("parses");

        // Then rollover and day window match the spec's defaults
        assert_eq!(day_config.rollover, TimeOfDay::new(4, 0).expect("valid"));
        assert_eq!(
            day_config.day_window,
            TimeWindow::new(
                TimeOfDay::new(9, 0).expect("valid"),
                TimeOfDay::new(18, 0).expect("valid"),
            )
            .expect("valid window")
        );
    }

    #[test]
    fn an_unparsable_rollover_string_fails_loudly() {
        // Given a config row with a malformed rollover time
        let store_config = store::Config {
            day_rollover: "not-a-time".to_string(),
            ..sample_store_config()
        };

        // When converting it into a domain DayConfig
        let result = DayConfig::try_from(&store_config);

        // Then it fails loudly rather than silently defaulting
        assert!(matches!(result, Err(DomainError::UnparsableTimeOfDay(_))));
    }

    #[test]
    fn a_degenerate_day_window_fails_loudly() {
        // Given a config row where the day window start and end are identical
        let store_config = store::Config {
            day_window_start: "09:00".to_string(),
            day_window_end: "09:00".to_string(),
            ..sample_store_config()
        };

        // When converting it into a domain DayConfig
        let result = DayConfig::try_from(&store_config);

        // Then it fails loudly rather than accepting a window that bounds nothing
        assert!(matches!(result, Err(DomainError::DegenerateTimeWindow)));
    }
}
