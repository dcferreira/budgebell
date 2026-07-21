use serde::{Deserialize, Serialize};

use crate::store::TriggerKind;

use super::error::DomainError;
use super::time::{TimeOfDay, Weekday};

/// How an at-time schedule repeats (design spec §4.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Recurrence {
    Daily,
    Weekdays,
    SpecificWeekdays { days: Vec<Weekday> },
}

impl Recurrence {
    /// Validates the recurrence — `SpecificWeekdays` must name at least one day.
    fn validate(&self) -> Result<(), DomainError> {
        match self {
            Recurrence::SpecificWeekdays { days } if days.is_empty() => {
                Err(DomainError::EmptySpecificWeekdays)
            }
            Recurrence::Daily | Recurrence::Weekdays | Recurrence::SpecificWeekdays { .. } => {
                Ok(())
            }
        }
    }
}

/// The config for an at-time schedule trigger — persisted as the habit's
/// `trigger_config_json` when `trigger_kind = schedule-at-time`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AtTimeConfig {
    pub time: TimeOfDay,
    pub recurrence: Recurrence,
    pub expires_at_day_end: bool,
}

/// The config for a weekly-count schedule trigger — persisted as the habit's
/// `trigger_config_json` when `trigger_kind = schedule-weekly-count`.
///
/// Implemented as a thin variant of time-of-day (design spec §4.2): an
/// optional preferred time, capped to `count` auto-chosen days per week.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeeklyCountConfig {
    pub count: u8,
    pub preferred_time: Option<TimeOfDay>,
    pub expires_at_day_end: bool,
}

/// One of the two schedule forms a habit's trigger may take (design spec §4.2).
#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleTrigger {
    AtTime(AtTimeConfig),
    WeeklyCount(WeeklyCountConfig),
}

/// A habit has exactly one trigger (design spec §4.2) — enforced structurally
/// here rather than via nullable columns, unlike the store's flat row shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Trigger {
    /// The habit belongs to a rotation, with `weight` biasing the picker.
    RotationMember {
        weight: u32,
    },
    Schedule(ScheduleTrigger),
}

const MIN_WEEKLY_COUNT: u8 = 1;
const MAX_WEEKLY_COUNT: u8 = 7;

impl Trigger {
    /// Builds a rotation-member trigger, rejecting a zero weight.
    pub fn rotation_member(weight: u32) -> Result<Self, DomainError> {
        if weight == 0 {
            return Err(DomainError::ZeroWeight);
        }
        Ok(Self::RotationMember { weight })
    }

    /// Builds an at-time schedule trigger.
    pub fn at_time(
        time: TimeOfDay,
        recurrence: Recurrence,
        expires_at_day_end: bool,
    ) -> Result<Self, DomainError> {
        recurrence.validate()?;
        Ok(Self::Schedule(ScheduleTrigger::AtTime(AtTimeConfig {
            time,
            recurrence,
            expires_at_day_end,
        })))
    }

    /// Builds a weekly-count schedule trigger, rejecting a count outside 1-7.
    pub fn weekly_count(
        count: u8,
        preferred_time: Option<TimeOfDay>,
        expires_at_day_end: bool,
    ) -> Result<Self, DomainError> {
        if !(MIN_WEEKLY_COUNT..=MAX_WEEKLY_COUNT).contains(&count) {
            return Err(DomainError::WeeklyCountOutOfRange(count));
        }
        Ok(Self::Schedule(ScheduleTrigger::WeeklyCount(
            WeeklyCountConfig {
                count,
                preferred_time,
                expires_at_day_end,
            },
        )))
    }

    /// The store's discriminant for this trigger's kind.
    pub fn kind(&self) -> TriggerKind {
        match self {
            Trigger::RotationMember { .. } => TriggerKind::RotationMember,
            Trigger::Schedule(ScheduleTrigger::AtTime(_)) => TriggerKind::ScheduleAtTime,
            Trigger::Schedule(ScheduleTrigger::WeeklyCount(_)) => TriggerKind::ScheduleWeeklyCount,
        }
    }

    /// The store's `weight` column value — `Some` only for rotation members.
    pub fn weight_column(&self) -> Option<i64> {
        match self {
            Trigger::RotationMember { weight } => Some(*weight as i64),
            Trigger::Schedule(_) => None,
        }
    }

    /// Serialises this trigger's detail into the store's `trigger_config_json`
    /// column. A rotation-member trigger carries no extra detail beyond its
    /// weight (already in its own column), so it serialises to `"{}"`.
    pub fn config_json(&self) -> Result<String, DomainError> {
        let json = match self {
            Trigger::RotationMember { .. } => "{}".to_string(),
            Trigger::Schedule(ScheduleTrigger::AtTime(config)) => serde_json::to_string(config)?,
            Trigger::Schedule(ScheduleTrigger::WeeklyCount(config)) => {
                serde_json::to_string(config)?
            }
        };
        Ok(json)
    }

    /// Reconstructs a trigger from the store's row shape: `trigger_kind`
    /// selects which config the `trigger_config_json` blob decodes as, and
    /// `weight` supplies a rotation member's weight.
    pub fn from_store_parts(
        kind: TriggerKind,
        config_json: &str,
        weight: Option<i64>,
    ) -> Result<Self, DomainError> {
        match kind {
            TriggerKind::RotationMember => {
                let weight = weight.unwrap_or(0);
                if weight <= 0 {
                    return Err(DomainError::ZeroWeight);
                }
                Self::rotation_member(weight as u32)
            }
            TriggerKind::ScheduleAtTime => {
                let config: AtTimeConfig = serde_json::from_str(config_json)?;
                Self::at_time(config.time, config.recurrence, config.expires_at_day_end)
            }
            TriggerKind::ScheduleWeeklyCount => {
                let config: WeeklyCountConfig = serde_json::from_str(config_json)?;
                Self::weekly_count(
                    config.count,
                    config.preferred_time,
                    config.expires_at_day_end,
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rotation_member_trigger_reports_its_kind_and_weight_column() {
        // Given a rotation-member trigger
        let trigger = Trigger::rotation_member(2).expect("valid weight");

        // Then its store kind and weight column match
        assert_eq!(trigger.kind(), TriggerKind::RotationMember);
        assert_eq!(trigger.weight_column(), Some(2));
    }

    #[test]
    fn a_zero_weight_rotation_member_is_rejected() {
        // Given a weight of zero
        // When building a rotation-member trigger
        let result = Trigger::rotation_member(0);

        // Then it fails loudly rather than silently accepting a useless weight
        assert!(matches!(result, Err(DomainError::ZeroWeight)));
    }

    #[test]
    fn a_rotation_member_trigger_serialises_to_an_empty_json_object() {
        // Given a rotation-member trigger (its weight lives in its own column)
        let trigger = Trigger::rotation_member(1).expect("valid weight");

        // When serialising its config JSON
        let json = trigger.config_json().expect("serialises");

        // Then no extra detail is carried in the JSON blob
        assert_eq!(json, "{}");
    }

    #[test]
    fn an_at_time_trigger_round_trips_through_the_stores_kind_and_json() {
        // Given an at-time trigger — "09:00 daily", expiring at day end
        let time = TimeOfDay::new(9, 0).expect("valid time");
        let trigger = Trigger::at_time(time, Recurrence::Daily, true).expect("valid trigger");

        // When serialised into the store's (kind, json, weight) shape
        let kind = trigger.kind();
        let json = trigger.config_json().expect("serialises");
        let weight = trigger.weight_column();

        // Then the kind and weight column match the store's expectations
        assert_eq!(kind, TriggerKind::ScheduleAtTime);
        assert_eq!(weight, None);

        // And reconstructing from those parts reproduces the original trigger
        let rebuilt = Trigger::from_store_parts(kind, &json, weight).expect("deserialises");
        assert_eq!(rebuilt, trigger);
    }

    #[test]
    fn a_weekdays_recurrence_round_trips_through_json() {
        // Given a "weekdays" at-time trigger with no expiry
        let time = TimeOfDay::new(8, 30).expect("valid time");
        let trigger = Trigger::at_time(time, Recurrence::Weekdays, false).expect("valid trigger");

        // When round-tripped through the store's (kind, json, weight) shape
        let json = trigger.config_json().expect("serialises");
        let rebuilt = Trigger::from_store_parts(trigger.kind(), &json, None).expect("deserialises");

        // Then it reproduces the original trigger exactly
        assert_eq!(rebuilt, trigger);
    }

    #[test]
    fn a_specific_weekdays_recurrence_round_trips_through_json() {
        // Given a "specific weekdays" at-time trigger for Monday and Thursday
        let time = TimeOfDay::new(7, 0).expect("valid time");
        let recurrence = Recurrence::SpecificWeekdays {
            days: vec![Weekday::Monday, Weekday::Thursday],
        };
        let trigger = Trigger::at_time(time, recurrence, true).expect("valid trigger");

        // When round-tripped through the store's (kind, json, weight) shape
        let json = trigger.config_json().expect("serialises");
        let rebuilt = Trigger::from_store_parts(trigger.kind(), &json, None).expect("deserialises");

        // Then it reproduces the original trigger exactly
        assert_eq!(rebuilt, trigger);
    }

    #[test]
    fn an_empty_specific_weekdays_list_is_rejected() {
        // Given a "specific weekdays" recurrence naming no days
        let time = TimeOfDay::new(7, 0).expect("valid time");
        let recurrence = Recurrence::SpecificWeekdays { days: vec![] };

        // When building an at-time trigger from it
        let result = Trigger::at_time(time, recurrence, false);

        // Then it fails loudly rather than accepting a schedule that never fires
        assert!(matches!(result, Err(DomainError::EmptySpecificWeekdays)));
    }

    #[test]
    fn a_weekly_count_trigger_with_a_preferred_time_round_trips_through_json() {
        // Given a "3x / week" trigger with a preferred time of 17:00 (design spec §7)
        let preferred = TimeOfDay::new(17, 0).expect("valid time");
        let trigger = Trigger::weekly_count(3, Some(preferred), false).expect("valid weekly count");

        // When serialised into the store's (kind, json, weight) shape
        let kind = trigger.kind();
        let json = trigger.config_json().expect("serialises");
        let weight = trigger.weight_column();

        // Then the kind and weight column match the store's expectations
        assert_eq!(kind, TriggerKind::ScheduleWeeklyCount);
        assert_eq!(weight, None);

        // And reconstructing from those parts reproduces the original trigger
        let rebuilt = Trigger::from_store_parts(kind, &json, weight).expect("deserialises");
        assert_eq!(rebuilt, trigger);
    }

    #[test]
    fn a_weekly_count_trigger_without_a_preferred_time_round_trips_through_json() {
        // Given a weekly-count trigger with no preferred time — the scheduler
        // auto-chooses a time within the global day window (design spec §4.7 D)
        let trigger = Trigger::weekly_count(2, None, true).expect("valid weekly count");

        // When round-tripped through the store's (kind, json, weight) shape
        let json = trigger.config_json().expect("serialises");
        let rebuilt = Trigger::from_store_parts(trigger.kind(), &json, None).expect("deserialises");

        // Then it reproduces the original trigger exactly
        assert_eq!(rebuilt, trigger);
    }

    #[test]
    fn a_weekly_count_of_zero_is_rejected() {
        // Given a count of zero times per week
        // When building a weekly-count trigger
        let result = Trigger::weekly_count(0, None, false);

        // Then it fails loudly rather than accepting a meaningless schedule
        assert!(matches!(result, Err(DomainError::WeeklyCountOutOfRange(0))));
    }

    #[test]
    fn a_weekly_count_above_seven_is_rejected() {
        // Given a count above the number of days in a week
        // When building a weekly-count trigger
        let result = Trigger::weekly_count(8, None, false);

        // Then it fails loudly rather than silently capping it
        assert!(matches!(result, Err(DomainError::WeeklyCountOutOfRange(8))));
    }

    #[test]
    fn malformed_trigger_config_json_fails_loudly_rather_than_falling_back() {
        // Given a store row claiming an at-time trigger but carrying invalid JSON
        // When reconstructing the trigger from it
        let result = Trigger::from_store_parts(TriggerKind::ScheduleAtTime, "not json", None);

        // Then it surfaces the JSON error rather than silently defaulting
        assert!(matches!(result, Err(DomainError::Json(_))));
    }
}
