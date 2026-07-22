//! `complete_habit` / `skip_habit` / `snooze_habit` — logging an event and,
//! for a schedule-triggered habit, resolving the fire that produced it so it
//! doesn't also expire at rollover (design spec §4.2).
//!
//! Assumption (not spelled out verbatim in the design spec): both Done and
//! Skipped resolve today's occurrence — the user has acted on it either way,
//! so it should not *also* be logged `expired` at rollover. Snoozing
//! deliberately leaves the fire unresolved: a snoozed habit the user never
//! comes back to still expires, same as if it had been ignored outright.

use chrono::NaiveDateTime;

use crate::domain::TimeOfDay;
use crate::scheduler::{self, HabitId, ScheduledFire, ScheduledHabitState, SchedulerState};
use crate::store::{self, EventAction, NewEvent, Store, TriggerKind};

use super::error::CommandError;

/// Logs `action` against `habit_id`, then applies any resulting scheduler
/// state transition. `shown_at` is the instant the toast was shown for this
/// occurrence, if known — the runtime reads it from the current due
/// occurrence in `AppState` (design spec §3.8/§4.5) so `done` events carry
/// enough to compute a duration.
pub fn record_action(
    store: &Store,
    scheduler_state: &mut SchedulerState,
    habit_id: i64,
    action: EventAction,
    now: NaiveDateTime,
    rollover: TimeOfDay,
    shown_at: Option<NaiveDateTime>,
) -> Result<(), CommandError> {
    let habit_row = find_habit(store, habit_id)?;
    store.append_event(&NewEvent {
        habit_id,
        action,
        at: now.and_utc().timestamp(),
        shown_at: shown_at.map(|instant| instant.and_utc().timestamp()),
    })?;

    if matches!(action, EventAction::Done | EventAction::Skipped) {
        resolve_scheduled_fire(scheduler_state, &habit_row, action, now, rollover);
    }
    Ok(())
}

fn find_habit(store: &Store, habit_id: i64) -> Result<store::Habit, CommandError> {
    store
        .list_habits()?
        .into_iter()
        .find(|row| row.id == habit_id)
        .ok_or(CommandError::HabitNotFound(habit_id))
}

/// Marks a schedule-triggered habit's most recent fire resolved. Rotation
/// members carry no such state — `RotationLastShown` has no `completed`
/// notion — so there is nothing further to do for them.
fn resolve_scheduled_fire(
    scheduler_state: &mut SchedulerState,
    habit_row: &store::Habit,
    action: EventAction,
    now: NaiveDateTime,
    rollover: TimeOfDay,
) {
    if habit_row.trigger_kind == TriggerKind::RotationMember {
        return;
    }

    let id = HabitId(habit_row.id);
    let entry = scheduler_state.scheduled_habits.entry(id).or_default();
    entry.last_fire = Some(ScheduledFire {
        fired_at: entry.last_fire.map_or(now, |fire| fire.fired_at),
        completed: true,
    });

    if action == EventAction::Done && habit_row.trigger_kind == TriggerKind::ScheduleWeeklyCount {
        record_weekly_completion(entry, now, rollover);
    }
}

/// Increments the weekly-count cap's completion tally, resetting it first if
/// the previously recorded week has since rolled over (design spec §4.7
/// example D).
fn record_weekly_completion(
    entry: &mut ScheduledHabitState,
    now: NaiveDateTime,
    rollover: TimeOfDay,
) {
    let week = scheduler::week_start(scheduler::rollover_day(now, rollover));
    if entry.week_of == Some(week) {
        entry.weekly_completions += 1;
    } else {
        entry.week_of = Some(week);
        entry.weekly_completions = 1;
    }
}

#[cfg(test)]
mod tests {
    use crate::store::{Category, NewHabit};

    use super::*;

    fn rollover() -> TimeOfDay {
        TimeOfDay::new(4, 0).expect("valid time")
    }

    fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(year, month, day)
            .expect("valid date")
            .and_hms_opt(hour, minute, 0)
            .expect("valid time")
    }

    fn insert_rotation_member(store: &Store) -> i64 {
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
                rotation_id: None,
                created_at: 0,
            })
            .expect("insert succeeds")
    }

    fn insert_at_time_habit(store: &Store) -> i64 {
        store
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
            .expect("insert succeeds")
    }

    fn insert_weekly_count_habit(store: &Store) -> i64 {
        store
            .insert_habit(&NewHabit {
                name: "Strength session".to_string(),
                instructions: "Bridges -> hip thrusts".to_string(),
                media_path: None,
                category: Category::Exercise,
                enabled: true,
                trigger_kind: TriggerKind::ScheduleWeeklyCount,
                trigger_config_json:
                    r#"{"count":3,"preferred_time":{"hour":17,"minute":0},"expires_at_day_end":false}"#
                        .to_string(),
                weight: None,
                rotation_id: None,
                created_at: 0,
            })
            .expect("insert succeeds")
    }

    #[test]
    fn completing_an_unknown_habit_fails_loudly() {
        // Given a store with no habits at all
        let store = Store::open_in_memory().expect("in-memory store opens");
        let mut state = SchedulerState::default();

        // When completing a habit id that doesn't exist
        let result = record_action(
            &store,
            &mut state,
            999,
            EventAction::Done,
            dt(2026, 7, 21, 10, 0),
            rollover(),
            None,
        );

        // Then it fails loudly rather than silently logging an orphaned event
        assert!(matches!(result, Err(CommandError::HabitNotFound(999))));
    }

    #[test]
    fn completing_a_rotation_member_only_logs_the_event() {
        // Given a rotation-member habit
        let store = Store::open_in_memory().expect("in-memory store opens");
        let habit_id = insert_rotation_member(&store);
        let mut state = SchedulerState::default();

        // When it is completed
        record_action(
            &store,
            &mut state,
            habit_id,
            EventAction::Done,
            dt(2026, 7, 21, 10, 0),
            rollover(),
            None,
        )
        .expect("succeeds");

        // Then a Done event is logged, and no scheduled-habit state is created
        let events = store.list_events().expect("list succeeds");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, EventAction::Done);
        assert!(state.scheduled_habits.is_empty());
    }

    #[test]
    fn completing_a_habit_with_a_known_shown_at_logs_it_for_duration_computation() {
        // Given a rotation-member habit whose toast was shown 108 seconds
        // before it was marked done (design spec §3.8)
        let store = Store::open_in_memory().expect("in-memory store opens");
        let habit_id = insert_rotation_member(&store);
        let mut state = SchedulerState::default();
        let shown_at = dt(2026, 7, 21, 10, 0);
        let done_at = dt(2026, 7, 21, 10, 3);

        // When it is completed, passing the toast's shown_at through
        record_action(
            &store,
            &mut state,
            habit_id,
            EventAction::Done,
            done_at,
            rollover(),
            Some(shown_at),
        )
        .expect("succeeds");

        // Then the logged event's duration is exactly done_at − shown_at
        let events = store.list_events().expect("list succeeds");
        assert_eq!(events[0].done_duration_secs(), Some(180));
    }

    #[test]
    fn completing_an_at_time_habit_resolves_its_fire_so_it_will_not_expire() {
        // Given an at-time habit that fired this morning and hasn't been
        // resolved yet
        let store = Store::open_in_memory().expect("in-memory store opens");
        let habit_id = insert_at_time_habit(&store);
        let mut state = SchedulerState::default();
        state.scheduled_habits.insert(
            HabitId(habit_id),
            ScheduledHabitState {
                last_fire: Some(ScheduledFire {
                    fired_at: dt(2026, 7, 21, 9, 0),
                    completed: false,
                }),
                ..Default::default()
            },
        );

        // When it is completed
        record_action(
            &store,
            &mut state,
            habit_id,
            EventAction::Done,
            dt(2026, 7, 21, 9, 5),
            rollover(),
            None,
        )
        .expect("succeeds");

        // Then its fire is marked completed
        let fire = state.scheduled_habits[&HabitId(habit_id)]
            .last_fire
            .expect("fire recorded");
        assert!(fire.completed);
    }

    #[test]
    fn skipping_an_at_time_habit_also_resolves_its_fire() {
        // Given an at-time habit that fired, unresolved
        let store = Store::open_in_memory().expect("in-memory store opens");
        let habit_id = insert_at_time_habit(&store);
        let mut state = SchedulerState::default();
        state.scheduled_habits.insert(
            HabitId(habit_id),
            ScheduledHabitState {
                last_fire: Some(ScheduledFire {
                    fired_at: dt(2026, 7, 21, 9, 0),
                    completed: false,
                }),
                ..Default::default()
            },
        );

        // When it is skipped
        record_action(
            &store,
            &mut state,
            habit_id,
            EventAction::Skipped,
            dt(2026, 7, 21, 9, 5),
            rollover(),
            None,
        )
        .expect("succeeds");

        // Then its fire is resolved too — skipping is a deliberate decision,
        // not an ignored occurrence
        assert!(
            state.scheduled_habits[&HabitId(habit_id)]
                .last_fire
                .expect("fire recorded")
                .completed
        );
    }

    #[test]
    fn snoozing_leaves_the_fire_unresolved() {
        // Given an at-time habit that fired, unresolved
        let store = Store::open_in_memory().expect("in-memory store opens");
        let habit_id = insert_at_time_habit(&store);
        let mut state = SchedulerState::default();
        state.scheduled_habits.insert(
            HabitId(habit_id),
            ScheduledHabitState {
                last_fire: Some(ScheduledFire {
                    fired_at: dt(2026, 7, 21, 9, 0),
                    completed: false,
                }),
                ..Default::default()
            },
        );

        // When it is snoozed
        record_action(
            &store,
            &mut state,
            habit_id,
            EventAction::Snoozed,
            dt(2026, 7, 21, 9, 5),
            rollover(),
            None,
        )
        .expect("succeeds");

        // Then the fire stays unresolved — a snoozed habit still expires if
        // nothing else resolves it
        assert!(
            !state.scheduled_habits[&HabitId(habit_id)]
                .last_fire
                .expect("fire recorded")
                .completed
        );

        // And the snooze itself is logged
        let events = store.list_events().expect("list succeeds");
        assert_eq!(events[0].action, EventAction::Snoozed);
    }

    #[test]
    fn completing_a_weekly_count_habit_increments_this_weeks_tally() {
        // Given a weekly-count habit with no completions logged yet this week
        let store = Store::open_in_memory().expect("in-memory store opens");
        let habit_id = insert_weekly_count_habit(&store);
        let mut state = SchedulerState::default();

        // When it is completed on a Tuesday
        record_action(
            &store,
            &mut state,
            habit_id,
            EventAction::Done,
            dt(2026, 7, 21, 17, 0),
            rollover(),
            None,
        )
        .expect("succeeds");

        // Then this week's tally is 1
        let entry = &state.scheduled_habits[&HabitId(habit_id)];
        assert_eq!(entry.weekly_completions, 1);

        // And completing it again later the same week increments it further
        record_action(
            &store,
            &mut state,
            habit_id,
            EventAction::Done,
            dt(2026, 7, 23, 17, 0),
            rollover(),
            None,
        )
        .expect("succeeds");
        assert_eq!(
            state.scheduled_habits[&HabitId(habit_id)].weekly_completions,
            2
        );
    }

    #[test]
    fn completing_a_weekly_count_habit_in_a_new_week_resets_the_tally() {
        // Given a weekly-count habit that already hit 3 completions last week
        let store = Store::open_in_memory().expect("in-memory store opens");
        let habit_id = insert_weekly_count_habit(&store);
        let mut state = SchedulerState::default();
        state.scheduled_habits.insert(
            HabitId(habit_id),
            ScheduledHabitState {
                weekly_completions: 3,
                week_of: Some(scheduler::week_start(scheduler::rollover_day(
                    dt(2026, 7, 14, 17, 0),
                    rollover(),
                ))),
                ..Default::default()
            },
        );

        // When it is completed in the new week
        record_action(
            &store,
            &mut state,
            habit_id,
            EventAction::Done,
            dt(2026, 7, 21, 17, 0),
            rollover(),
            None,
        )
        .expect("succeeds");

        // Then the tally resets to 1 for the new week rather than accumulating
        assert_eq!(
            state.scheduled_habits[&HabitId(habit_id)].weekly_completions,
            1
        );
    }

    #[test]
    fn skipping_a_weekly_count_habit_does_not_count_towards_the_tally() {
        // Given a weekly-count habit with no completions yet
        let store = Store::open_in_memory().expect("in-memory store opens");
        let habit_id = insert_weekly_count_habit(&store);
        let mut state = SchedulerState::default();

        // When it is skipped rather than done
        record_action(
            &store,
            &mut state,
            habit_id,
            EventAction::Skipped,
            dt(2026, 7, 21, 17, 0),
            rollover(),
            None,
        )
        .expect("succeeds");

        // Then the tally is unaffected — skipping resolves the occurrence
        // but doesn't count as a completion
        assert_eq!(
            state.scheduled_habits[&HabitId(habit_id)].weekly_completions,
            0
        );
    }
}
