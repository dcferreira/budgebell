//! Request/response types for the MCP tools (design spec §6), plus their
//! conversions to and from the domain model and the store's row shapes.
//!
//! These mirror the domain types rather than reusing them directly because
//! the rmcp SDK needs `schemars::JsonSchema` on every tool parameter, and the
//! domain/store layers deliberately don't depend on schemars. Keeping the
//! wire shapes here also lets the tool surface stay ergonomic for an LLM
//! caller (times as "HH:MM" strings, sensible defaults) without leaking that
//! into the core.

use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::domain::{Habit as DomainHabit, Recurrence, TimeOfDay, Trigger, Weekday};
use crate::stats::{DayLog, DaySummary, SedentaryGap};
use crate::store::{self, Category, Event, EventAction, Habit as StoreHabit, LoggedEvent};

use super::error::McpToolError;

fn default_true() -> bool {
    true
}

/// The content category, mirroring [`store::Category`] with a JSON schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CategoryDto {
    Exercise,
    General,
}

impl From<CategoryDto> for Category {
    fn from(value: CategoryDto) -> Self {
        match value {
            CategoryDto::Exercise => Category::Exercise,
            CategoryDto::General => Category::General,
        }
    }
}

impl From<Category> for CategoryDto {
    fn from(value: Category) -> Self {
        match value {
            Category::Exercise => CategoryDto::Exercise,
            Category::General => CategoryDto::General,
        }
    }
}

/// A logged outcome, mirroring [`store::EventAction`] with a JSON schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ActionDto {
    Done,
    Expired,
    Skipped,
    Snoozed,
}

impl From<ActionDto> for EventAction {
    fn from(value: ActionDto) -> Self {
        match value {
            ActionDto::Done => EventAction::Done,
            ActionDto::Expired => EventAction::Expired,
            ActionDto::Skipped => EventAction::Skipped,
            ActionDto::Snoozed => EventAction::Snoozed,
        }
    }
}

impl From<EventAction> for ActionDto {
    fn from(value: EventAction) -> Self {
        match value {
            EventAction::Done => ActionDto::Done,
            EventAction::Expired => ActionDto::Expired,
            EventAction::Skipped => ActionDto::Skipped,
            EventAction::Snoozed => ActionDto::Snoozed,
        }
    }
}

/// A weekday, mirroring [`domain::Weekday`](crate::domain::Weekday).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum WeekdayDto {
    Monday,
    Tuesday,
    Wednesday,
    Thursday,
    Friday,
    Saturday,
    Sunday,
}

impl From<WeekdayDto> for Weekday {
    fn from(value: WeekdayDto) -> Self {
        match value {
            WeekdayDto::Monday => Weekday::Monday,
            WeekdayDto::Tuesday => Weekday::Tuesday,
            WeekdayDto::Wednesday => Weekday::Wednesday,
            WeekdayDto::Thursday => Weekday::Thursday,
            WeekdayDto::Friday => Weekday::Friday,
            WeekdayDto::Saturday => Weekday::Saturday,
            WeekdayDto::Sunday => Weekday::Sunday,
        }
    }
}

/// How an at-time schedule repeats, mirroring [`domain::Recurrence`](Recurrence).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RecurrenceDto {
    Daily,
    Weekdays,
    SpecificWeekdays { days: Vec<WeekdayDto> },
}

impl From<RecurrenceDto> for Recurrence {
    fn from(value: RecurrenceDto) -> Self {
        match value {
            RecurrenceDto::Daily => Recurrence::Daily,
            RecurrenceDto::Weekdays => Recurrence::Weekdays,
            RecurrenceDto::SpecificWeekdays { days } => Recurrence::SpecificWeekdays {
                days: days.into_iter().map(Weekday::from).collect(),
            },
        }
    }
}

/// The single trigger a habit carries (design spec §4.2). `rotation_id` is a
/// store-level relationship, meaningful only for a rotation member.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum TriggerDto {
    RotationMember {
        weight: u32,
        #[serde(default)]
        rotation_id: Option<i64>,
    },
    ScheduleAtTime {
        /// Time of day in "HH:MM" (24-hour) form.
        time: String,
        recurrence: RecurrenceDto,
        #[serde(default)]
        expires_at_day_end: bool,
    },
    ScheduleWeeklyCount {
        count: u8,
        /// Optional preferred time of day in "HH:MM" form.
        #[serde(default)]
        preferred_time: Option<String>,
        #[serde(default)]
        expires_at_day_end: bool,
    },
}

impl TriggerDto {
    /// Converts to a validated domain [`Trigger`] plus the rotation membership
    /// (if any). Time parsing and range checks fail loudly as [`McpToolError`].
    pub fn into_trigger(self) -> Result<(Trigger, Option<i64>), McpToolError> {
        match self {
            TriggerDto::RotationMember {
                weight,
                rotation_id,
            } => Ok((Trigger::rotation_member(weight)?, rotation_id)),
            TriggerDto::ScheduleAtTime {
                time,
                recurrence,
                expires_at_day_end,
            } => {
                let time = TimeOfDay::from_str(&time)?;
                Ok((
                    Trigger::at_time(time, recurrence.into(), expires_at_day_end)?,
                    None,
                ))
            }
            TriggerDto::ScheduleWeeklyCount {
                count,
                preferred_time,
                expires_at_day_end,
            } => {
                let preferred_time = preferred_time
                    .map(|raw| TimeOfDay::from_str(&raw))
                    .transpose()?;
                Ok((
                    Trigger::weekly_count(count, preferred_time, expires_at_day_end)?,
                    None,
                ))
            }
        }
    }
}

/// `add_habit` input (design spec §6). `enabled` defaults to `true` so a new
/// habit is live unless the caller says otherwise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AddHabitRequest {
    pub name: String,
    pub instructions: String,
    #[serde(default)]
    pub media_path: Option<String>,
    pub category: CategoryDto,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub trigger: TriggerDto,
}

impl AddHabitRequest {
    /// Builds the store-insertable row from this request at time `created_at`.
    pub fn into_new_habit(self, created_at: i64) -> Result<store::NewHabit, McpToolError> {
        let (trigger, rotation_id) = self.trigger.into_trigger()?;
        let habit = DomainHabit::new(
            self.name,
            self.instructions,
            self.media_path,
            self.category.into(),
            self.enabled,
            trigger,
        )?;
        Ok(habit.to_new_habit(created_at, rotation_id)?)
    }
}

/// `add_habit` output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AddHabitResponse {
    pub id: i64,
}

/// A habit as returned by `list_habits` — the store's flat row, with enums
/// rendered as strings so the shape is stable over the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HabitDto {
    pub id: i64,
    pub name: String,
    pub instructions: String,
    pub media_path: Option<String>,
    pub category: CategoryDto,
    pub enabled: bool,
    pub trigger_kind: String,
    pub trigger_config_json: String,
    pub weight: Option<i64>,
    pub rotation_id: Option<i64>,
    pub created_at: i64,
}

impl From<StoreHabit> for HabitDto {
    fn from(habit: StoreHabit) -> Self {
        // Reuse the store enum's own kebab-case rendering for the kind so the
        // string never drifts from the persisted value.
        let trigger_kind = serde_json::to_value(habit.trigger_kind)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();
        Self {
            id: habit.id,
            name: habit.name,
            instructions: habit.instructions,
            media_path: habit.media_path,
            category: habit.category.into(),
            enabled: habit.enabled,
            trigger_kind,
            trigger_config_json: habit.trigger_config_json,
            weight: habit.weight,
            rotation_id: habit.rotation_id,
            created_at: habit.created_at,
        }
    }
}

/// `list_habits` output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ListHabitsResponse {
    pub habits: Vec<HabitDto>,
}

/// `update_habit` input. Only the provided content fields change; omitted
/// fields keep their stored value. The trigger is not edited here — that is a
/// larger operation and out of scope for v1's tool surface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UpdateHabitRequest {
    pub id: i64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub media_path: Option<String>,
    #[serde(default)]
    pub category: Option<CategoryDto>,
    #[serde(default)]
    pub enabled: Option<bool>,
}

/// `update_habit` output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UpdateHabitResponse {
    pub id: i64,
}

/// `disable_habit` input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DisableHabitRequest {
    pub id: i64,
}

/// `disable_habit` output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DisableHabitResponse {
    pub id: i64,
}

/// `query_log` input. All filters are optional and combine with AND; `since`
/// is inclusive and `until` exclusive (unix-second timestamps). `limit` caps
/// the result to the most recent N matching events.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QueryLogRequest {
    #[serde(default)]
    pub habit_id: Option<i64>,
    #[serde(default)]
    pub action: Option<ActionDto>,
    #[serde(default)]
    pub since: Option<i64>,
    #[serde(default)]
    pub until: Option<i64>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// A single logged event as returned by `query_log`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventDto {
    pub id: i64,
    pub habit_id: i64,
    pub action: ActionDto,
    pub at: i64,
}

impl From<Event> for EventDto {
    fn from(event: Event) -> Self {
        Self {
            id: event.id,
            habit_id: event.habit_id,
            action: event.action.into(),
            at: event.at,
        }
    }
}

/// `query_log` output, ordered chronologically (oldest first).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct QueryLogResponse {
    pub events: Vec<EventDto>,
}

/// `day_log` input (design spec §6.1): the rollover-day to fetch, as a
/// "YYYY-MM-DD" date string.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DayLogRequest {
    pub date: String,
}

/// A single logged event as returned by `day_log`, already joined with its
/// habit's name and category so the caller needs no further round-trip.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LoggedEventDto {
    pub id: i64,
    pub habit_id: i64,
    pub habit_name: String,
    pub category: CategoryDto,
    pub action: ActionDto,
    pub at: i64,
    pub shown_at: Option<i64>,
    pub duration_secs: Option<i64>,
}

impl From<LoggedEvent> for LoggedEventDto {
    fn from(logged: LoggedEvent) -> Self {
        Self {
            id: logged.event.id,
            habit_id: logged.event.habit_id,
            habit_name: logged.habit_name,
            category: logged.category.into(),
            duration_secs: logged.event.done_duration_secs(),
            action: logged.event.action.into(),
            at: logged.event.at,
            shown_at: logged.event.shown_at,
        }
    }
}

/// The day summary tile counts (design spec §3.9/§6.1): done/skipped counts,
/// total measured movement time, and adherence (`0` when there was neither a
/// done nor a skipped event).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DaySummaryDto {
    pub done_count: u32,
    pub skipped_count: u32,
    pub total_moving_secs: i64,
    pub adherence_pct: f64,
}

impl From<DaySummary> for DaySummaryDto {
    fn from(summary: DaySummary) -> Self {
        Self {
            done_count: summary.done_count,
            skipped_count: summary.skipped_count,
            total_moving_secs: summary.total_moving_secs,
            adherence_pct: summary.adherence_pct,
        }
    }
}

/// The longest sedentary gap (design spec §3.9/§6.1): the largest span
/// between movements, plus the instants it spanned.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SedentaryGapDto {
    pub duration_secs: i64,
    pub start: i64,
    pub end: i64,
}

impl From<SedentaryGap> for SedentaryGapDto {
    fn from(gap: SedentaryGap) -> Self {
        Self {
            duration_secs: gap.duration_secs,
            start: gap.start,
            end: gap.end,
        }
    }
}

/// `day_log` output (design spec §6.1): the requested rollover-day's events
/// plus the shared day summary and longest-sedentary-gap aggregation, so a
/// locally-running LLM can read adherence and the daily picture in one call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DayLogResponse {
    pub date: String,
    pub events: Vec<LoggedEventDto>,
    pub summary: DaySummaryDto,
    pub longest_gap: SedentaryGapDto,
}

impl From<DayLog> for DayLogResponse {
    fn from(log: DayLog) -> Self {
        Self {
            date: log.date.format("%Y-%m-%d").to_string(),
            events: log.events.into_iter().map(LoggedEventDto::from).collect(),
            summary: log.summary.into(),
            longest_gap: log.longest_gap.into(),
        }
    }
}

/// `log_event` input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LogEventRequest {
    pub habit_id: i64,
    pub action: ActionDto,
    pub at: i64,
}

/// `log_event` output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LogEventResponse {
    pub id: i64,
}
