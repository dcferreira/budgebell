//! `complete_habit` / `skip_habit` / `snooze_habit` — logging an event and,
//! for a schedule-triggered habit, resolving the fire that produced it so it
//! doesn't also expire at rollover (design spec §4.2).
//!
//! Assumption (not spelled out verbatim in the design spec): both Done and
//! Skipped resolve today's occurrence — the user has acted on it either way,
//! so it should not *also* be logged `expired` at rollover. Snoozing
//! deliberately leaves the fire unresolved: a snoozed habit the user never
//! comes back to still expires, same as if it had been ignored outright.

use chrono::{DateTime, Utc};

use crate::clock::Zone;
use crate::domain::TimeOfDay;
use crate::scheduler::{self, HabitId, ScheduledFire, ScheduledHabitState, SchedulerState};
use crate::store::{self, EventAction, NewEvent, Store, TriggerKind};

use super::error::CommandError;

/// Logs `action` against `habit_id`, then applies any resulting scheduler
/// state transition. `shown_at` is the instant the toast was shown for this
/// occurrence, if known — the runtime reads it from the current due
/// occurrence in `AppState` (design spec §3.8/§4.5) so `done` events carry
/// enough to compute a duration. Both are stored as unix epoch seconds (UTC);
/// `zone` is where the `rollover` time-of-day is read.
#[allow(clippy::too_many_arguments)]
pub fn record_action(
    store: &Store,
    scheduler_state: &mut SchedulerState,
    habit_id: i64,
    action: EventAction,
    now: DateTime<Utc>,
    rollover: TimeOfDay,
    zone: Zone,
    shown_at: Option<DateTime<Utc>>,
) -> Result<(), CommandError> {
    let habit_row = find_habit(store, habit_id)?;
    store.append_event(&NewEvent {
        habit_id,
        action,
        at: now.timestamp(),
        shown_at: shown_at.map(|instant| instant.timestamp()),
    })?;

    // However long the nudge sat on screen, the next one waits a full
    // interval from now — a snooze included, since it means "not now".
    scheduler_state.rest_after_resolving(now);

    if matches!(action, EventAction::Done | EventAction::Skipped) {
        resolve_scheduled_fire(scheduler_state, &habit_row, action, now, rollover, zone);
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
    now: DateTime<Utc>,
    rollover: TimeOfDay,
    zone: Zone,
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
        record_weekly_completion(entry, now, rollover, zone);
    }
}

/// Increments the weekly-count cap's completion tally, resetting it first if
/// the previously recorded week has since rolled over (design spec §4.7
/// example D).
fn record_weekly_completion(
    entry: &mut ScheduledHabitState,
    now: DateTime<Utc>,
    rollover: TimeOfDay,
    zone: Zone,
) {
    let week = scheduler::week_start(scheduler::rollover_day(now, rollover, zone));
    if entry.week_of == Some(week) {
        entry.weekly_completions += 1;
    } else {
        entry.week_of = Some(week);
        entry.weekly_completions = 1;
    }
}

#[cfg(test)]
mod tests {
    use crate::clock::{london as dt, LONDON};
    use crate::store::{Category, NewHabit};

    use super::*;

    fn rollover() -> TimeOfDay {
        TimeOfDay::new(4, 0).expect("valid time")
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
            LONDON,
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
            LONDON,
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
    fn resolving_a_nudge_in_any_way_restarts_the_rest_from_that_moment() {
        for action in [
            EventAction::Done,
            EventAction::Skipped,
            EventAction::Snoozed,
        ] {
            // Given a nudge that has sat on screen since 11:10
            let store = Store::open_in_memory().expect("in-memory store opens");
            let habit_id = insert_rotation_member(&store);
            let mut state = SchedulerState::starting_at(dt(2026, 10, 1, 9, 0));

            // When it is finally resolved at 11:37
            record_action(
                &store,
                &mut state,
                habit_id,
                action,
                dt(2026, 10, 1, 11, 37),
                rollover(),
                LONDON,
                Some(dt(2026, 10, 1, 11, 10)),
            )
            .expect("succeeds");

            // Then the next nudge rests from the resolution, not the showing
            assert_eq!(state.rest_from, Some(dt(2026, 10, 1, 11, 37)), "{action:?}");
        }
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
            LONDON,
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
            LONDON,
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
            LONDON,
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
            LONDON,
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
            LONDON,
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
            LONDON,
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
                    LONDON,
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
            LONDON,
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
            LONDON,
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

    #[test]
    fn an_event_the_app_logs_and_the_mcp_day_log_agree_on_its_rollover_day() {
        // Given a configured store, and a drill done at 04:10 BST on
        // 2026-07-22 — past the 04:00 rollover locally, but 03:10 in UTC
        let store = Store::open_in_memory().expect("in-memory store opens");
        store
            .write_config(&crate::store::Config {
                day_rollover: "04:00".to_string(),
                day_window_start: "09:00".to_string(),
                day_window_end: "18:00".to_string(),
                calendar_pause_enabled: true,
                calendar_mode: crate::store::CalendarMode::WithOthers,
                idle_enabled: true,
                dnd_enabled: true,
                mic_pause_enabled: true,
                start_at_login: false,
            })
            .expect("write succeeds");
        let habit_id = insert_rotation_member(&store);
        let done_at = dt(2026, 7, 22, 4, 10);
        let now = dt(2026, 7, 22, 4, 50);

        // When the app logs it, and the MCP day_log reads that day 40 minutes
        // later (still before 04:00 in UTC)
        record_action(
            &store,
            &mut SchedulerState::default(),
            habit_id,
            EventAction::Done,
            done_at,
            rollover(),
            LONDON,
            None,
        )
        .expect("succeeds");
        let response = crate::mcp::handlers::day_log(
            &store,
            crate::mcp::dto::DayLogRequest {
                date: "2026-07-22".to_string(),
            },
            now,
            LONDON,
        )
        .expect("day_log succeeds");

        // Then the event is in that day, and `now` is seen as that same day:
        // the single movement's gap runs on to `now` rather than being absent
        assert_eq!(response.events.len(), 1);
        assert_eq!(response.events[0].at, done_at.timestamp());
        let gap = response.longest_gap.expect("today's gap runs to now");
        assert_eq!(gap.end, now.timestamp());
        assert_eq!(gap.duration_secs, 40 * 60);
    }
}
