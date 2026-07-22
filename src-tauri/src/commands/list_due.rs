//! `list_due` — the scheduler-facing edge: reads config + habits from the
//! store, calls the pure `schedule()`, then applies the resulting state
//! transitions (recording what was shown, logging + clearing expirations)
//! that the pure function deliberately leaves to its caller (design spec
//! §4.6).

use chrono::NaiveDateTime;

use crate::domain::{DayConfig, QuietState};
use crate::scheduler::{
    self, Expiration, HabitId, RotationInput, RotationLastShown, ScheduledFire, SchedulerState,
};
use crate::store::{EventAction, NewEvent, Store};

use super::build::build_scheduler_inputs;
use super::dto::{DecisionDto, DueHabitDto};
use super::error::CommandError;

/// Computes what's due right now and applies the resulting state
/// transitions. Takes `now`, `quiet_state` and `paused_until` as explicit
/// parameters so it is fully testable without a Tauri runtime.
pub fn list_due_impl(
    store: &Store,
    scheduler_state: &mut SchedulerState,
    now: NaiveDateTime,
    quiet_state: QuietState,
    paused_until: Option<NaiveDateTime>,
) -> Result<DecisionDto, CommandError> {
    let config = store.read_config()?.ok_or(CommandError::ConfigNotSet)?;
    let day_config = DayConfig::try_from(&config)?;
    let (scheduled_habits, rotations) = build_scheduler_inputs(store)?;
    let rng_seed = now.and_utc().timestamp() as u64;

    let decision = scheduler::schedule(
        &scheduled_habits,
        &rotations,
        now,
        quiet_state,
        day_config,
        scheduler_state,
        rng_seed,
    );

    apply_expirations(store, scheduler_state, &decision.expirations, now)?;

    // Manually paused nudges (design spec §3.7) suppress firing without
    // touching `SchedulerState` — the next call finds the same tick still
    // due and fires it then, exactly like the idle hold/re-arm behaviour
    // (§4.7 example E), so nothing is lost by pausing.
    let is_paused = paused_until.is_some_and(|until| now < until);
    let due_now = if is_paused { None } else { decision.due_now };
    if let Some(due) = &due_now {
        record_shown(scheduler_state, &rotations, due.habit_id, now);
    }

    Ok(DecisionDto {
        due_now: due_now.map(DueHabitDto::from),
        next_due: decision.next_due,
    })
}

/// Logs each expired occurrence and clears its recorded fire so the same
/// expiration isn't re-reported on the next call.
fn apply_expirations(
    store: &Store,
    scheduler_state: &mut SchedulerState,
    expirations: &[Expiration],
    now: NaiveDateTime,
) -> Result<(), CommandError> {
    for expiration in expirations {
        store.append_event(&NewEvent {
            habit_id: expiration.habit_id.0,
            action: EventAction::Expired,
            at: now.and_utc().timestamp(),
        })?;
        if let Some(state) = scheduler_state
            .scheduled_habits
            .get_mut(&expiration.habit_id)
        {
            state.last_fire = None;
        }
    }
    Ok(())
}

/// Records that `habit_id` fired right now, so the next call doesn't
/// immediately re-fire it: a picked rotation member updates that rotation's
/// `last_shown`; a scheduled habit updates its own `last_fire`.
fn record_shown(
    scheduler_state: &mut SchedulerState,
    rotations: &[RotationInput],
    habit_id: HabitId,
    now: NaiveDateTime,
) {
    let owning_rotation = rotations.iter().find(|rotation| {
        rotation
            .members
            .iter()
            .any(|member| member.habit_id == habit_id)
    });

    match owning_rotation {
        Some(rotation) => {
            scheduler_state
                .rotations
                .insert(rotation.id, RotationLastShown { habit_id, at: now });
        }
        None => {
            let entry = scheduler_state
                .scheduled_habits
                .entry(habit_id)
                .or_default();
            entry.last_fire = Some(ScheduledFire {
                fired_at: now,
                completed: false,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use crate::store::{
        CalendarMode, Category, Config, NewHabit, NewRotation, TriggerKind, WindowKind,
    };

    use super::*;

    fn dt(hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 7, 21)
            .expect("valid date")
            .and_hms_opt(hour, minute, 0)
            .expect("valid time")
    }

    fn default_config() -> Config {
        Config {
            day_rollover: "04:00".to_string(),
            day_window_start: "09:00".to_string(),
            day_window_end: "18:00".to_string(),
            calendar_pause_enabled: true,
            calendar_mode: CalendarMode::WithOthers,
            idle_enabled: true,
            dnd_enabled: true,
            start_at_login: false,
        }
    }

    fn store_with_rotation_of_one() -> Store {
        let store = Store::open_in_memory().expect("in-memory store opens");
        store
            .write_config(&default_config())
            .expect("write succeeds");
        let rotation_id = store
            .insert_rotation(&NewRotation {
                name: "Movement snacks".to_string(),
                interval_secs: 1_800,
                window_kind: WindowKind::InheritGlobal,
                window_start: None,
                window_end: None,
            })
            .expect("rotation insert succeeds");
        store
            .insert_habit(&NewHabit {
                name: "Lunge-and-reach".to_string(),
                instructions: "5 slow reps/leg".to_string(),
                media_path: None,
                category: Category::Exercise,
                enabled: true,
                trigger_kind: TriggerKind::RotationMember,
                trigger_config_json: "{}".to_string(),
                weight: Some(1),
                rotation_id: Some(rotation_id),
                created_at: 0,
            })
            .expect("insert succeeds");
        store
    }

    #[test]
    fn without_a_config_row_list_due_fails_loudly_rather_than_assuming_defaults() {
        // Given a store that has never had its config written
        let store = Store::open_in_memory().expect("in-memory store opens");
        let mut state = SchedulerState::default();

        // When listing due habits
        let result = list_due_impl(&store, &mut state, dt(10, 0), QuietState::all_clear(), None);

        // Then it fails loudly rather than silently assuming a default config
        assert!(matches!(result, Err(CommandError::ConfigNotSet)));
    }

    #[test]
    fn a_due_rotation_tick_fires_once_and_is_recorded_so_it_does_not_fire_again_immediately() {
        // Given a fresh rotation of one, inside its window
        let store = store_with_rotation_of_one();
        let mut state = SchedulerState::default();

        // When listing due habits at 10:00
        let first = list_due_impl(&store, &mut state, dt(10, 0), QuietState::all_clear(), None)
            .expect("succeeds");

        // Then the habit fires, and its rotation's last-shown state was recorded
        assert!(first.due_now.is_some());
        assert_eq!(state.rotations.len(), 1);

        // When listing again immediately after (before the next 30-minute tick)
        let second = list_due_impl(&store, &mut state, dt(10, 0), QuietState::all_clear(), None)
            .expect("succeeds");

        // Then it does not fire again straight away
        assert!(second.due_now.is_none());
    }

    #[test]
    fn a_manual_pause_suppresses_firing_without_losing_the_tick() {
        // Given a rotation tick that is due, but nudges are manually paused
        // until 10:30
        let store = store_with_rotation_of_one();
        let mut state = SchedulerState::default();
        let paused_until = Some(dt(10, 30));

        // When listing due habits at 10:00, while paused
        let paused = list_due_impl(
            &store,
            &mut state,
            dt(10, 0),
            QuietState::all_clear(),
            paused_until,
        )
        .expect("succeeds");

        // Then nothing fires, and no rotation state was recorded — the tick
        // is held, not lost
        assert!(paused.due_now.is_none());
        assert!(state.rotations.is_empty());

        // And once resumed, the very same tick fires
        let resumed = list_due_impl(&store, &mut state, dt(10, 0), QuietState::all_clear(), None)
            .expect("succeeds");
        assert!(resumed.due_now.is_some());
    }

    #[test]
    fn an_expired_scheduled_habit_is_logged_and_cleared_so_it_is_not_reported_twice() {
        // Given a daily 09:00 habit that fired yesterday and was never
        // actioned (design spec §4.7 example C)
        let store = Store::open_in_memory().expect("in-memory store opens");
        store
            .write_config(&default_config())
            .expect("write succeeds");
        let habit_id = store
            .insert_habit(&NewHabit {
                name: "Morning stretch".to_string(),
                instructions: "Full-body stretch".to_string(),
                media_path: None,
                category: Category::General,
                enabled: true,
                trigger_kind: TriggerKind::ScheduleAtTime,
                trigger_config_json:
                    r#"{"time":{"hour":9,"minute":0},"recurrence":{"kind":"daily"},"expires_at_day_end":true}"#
                        .to_string(),
                weight: None,
                rotation_id: None,
                created_at: 0,
            })
            .expect("insert succeeds");
        let mut state = SchedulerState::default();
        state.scheduled_habits.insert(
            HabitId(habit_id),
            crate::scheduler::ScheduledHabitState {
                last_fire: Some(ScheduledFire {
                    fired_at: NaiveDate::from_ymd_opt(2026, 7, 20)
                        .expect("valid date")
                        .and_hms_opt(9, 0, 0)
                        .expect("valid time"),
                    completed: false,
                }),
                ..Default::default()
            },
        );

        // When listing due habits after the next rollover
        list_due_impl(&store, &mut state, dt(5, 0), QuietState::all_clear(), None)
            .expect("succeeds");

        // Then an `expired` event was logged
        let events = store.list_events().expect("list succeeds");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, EventAction::Expired);

        // And a second call at the same instant does not log it again
        list_due_impl(&store, &mut state, dt(5, 0), QuietState::all_clear(), None)
            .expect("succeeds");
        assert_eq!(store.list_events().expect("list succeeds").len(), 1);
    }
}
