//! `list_due` — the scheduler-facing edge: reads config + habits from the
//! store, calls the pure `schedule()`, then applies the resulting state
//! transitions (recording what was shown, logging + clearing expirations)
//! that the pure function deliberately leaves to its caller (design spec
//! §4.6).

use chrono::{DateTime, Utc};

use crate::clock::Zone;
use crate::domain::{DayConfig, QuietState};
use crate::scheduler::{
    self, Expiration, HabitId, RotationInput, RotationLastShown, ScheduledFire, SchedulerState,
};
use crate::store::{EventAction, NewEvent, Store};

use super::build::build_scheduler_inputs;
use super::dto::{DecisionDto, DueHabitDto};
use super::error::CommandError;

/// Computes what's due right now and applies the resulting state
/// transitions. Takes `now`, the `zone` the day config is read in,
/// `quiet_state`, `paused_until` and `nudge_outstanding` as explicit
/// parameters so it is fully testable without a Tauri runtime.
pub fn list_due_impl(
    store: &Store,
    scheduler_state: &mut SchedulerState,
    now: DateTime<Utc>,
    zone: Zone,
    quiet_state: QuietState,
    paused_until: Option<DateTime<Utc>>,
    nudge_outstanding: bool,
) -> Result<DecisionDto, CommandError> {
    let config = store.read_config()?.ok_or(CommandError::ConfigNotSet)?;
    let day_config = DayConfig::try_from(&config)?;
    let (scheduled_habits, rotations) = build_scheduler_inputs(store)?;
    let rng_seed = now.timestamp() as u64;

    // Coming back (from idle, or after a gap such as suspend) restarts the
    // rest, so the next rotation tick waits a full interval from the return.
    scheduler_state.observe_presence(now, quiet_state.idle);

    let decision = scheduler::schedule(
        &scheduled_habits,
        &rotations,
        now,
        zone,
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
    // A nudge still on screen holds the next rotation tick the same way; a
    // fixed-time habit still fires at its time, so it can't silently miss
    // its day behind an ignored toast.
    let is_paused = paused_until.is_some_and(|until| now < until);
    let due_now = decision.due_now.filter(|due| {
        let held_behind_toast = nudge_outstanding && is_rotation_member(&rotations, due.habit_id);
        !is_paused && !held_behind_toast
    });
    if let Some(due) = &due_now {
        record_shown(scheduler_state, &rotations, due.habit_id, now);
    }

    Ok(DecisionDto {
        due_now: due_now.map(DueHabitDto::from),
        next_due: decision.next_due,
    })
}

fn is_rotation_member(rotations: &[RotationInput], habit_id: HabitId) -> bool {
    rotations.iter().any(|rotation| {
        rotation
            .members
            .iter()
            .any(|member| member.habit_id == habit_id)
    })
}

/// Logs each expired occurrence and clears its recorded fire so the same
/// expiration isn't re-reported on the next call.
fn apply_expirations(
    store: &Store,
    scheduler_state: &mut SchedulerState,
    expirations: &[Expiration],
    now: DateTime<Utc>,
) -> Result<(), CommandError> {
    for expiration in expirations {
        store.append_event(&NewEvent {
            habit_id: expiration.habit_id.0,
            action: EventAction::Expired,
            at: now.timestamp(),
            shown_at: None,
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
    now: DateTime<Utc>,
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
    use crate::clock::{london, LONDON};
    use crate::store::{
        CalendarMode, Category, Config, NewHabit, NewRotation, TriggerKind, WindowKind,
    };

    use super::*;

    fn dt(hour: u32, minute: u32) -> DateTime<Utc> {
        london(2026, 7, 21, hour, minute)
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
            mic_pause_enabled: true,
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
        let result = list_due_impl(
            &store,
            &mut state,
            dt(10, 0),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        );

        // Then it fails loudly rather than silently assuming a default config
        assert!(matches!(result, Err(CommandError::ConfigNotSet)));
    }

    #[test]
    fn a_due_rotation_tick_fires_once_and_is_recorded_so_it_does_not_fire_again_immediately() {
        // Given a fresh rotation of one, inside its window
        let store = store_with_rotation_of_one();
        let mut state = SchedulerState::default();

        // When listing due habits at 10:00
        let first = list_due_impl(
            &store,
            &mut state,
            dt(10, 0),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        )
        .expect("succeeds");

        // Then the habit fires, and its rotation's last-shown state was recorded
        assert!(first.due_now.is_some());
        assert_eq!(state.rotations.len(), 1);

        // When listing again immediately after (before the next 30-minute tick)
        let second = list_due_impl(
            &store,
            &mut state,
            dt(10, 0),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        )
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
            LONDON,
            QuietState::all_clear(),
            paused_until,
            false,
        )
        .expect("succeeds");

        // Then nothing fires, and no rotation state was recorded — the tick
        // is held, not lost
        assert!(paused.due_now.is_none());
        assert!(state.rotations.is_empty());

        // And once resumed, the very same tick fires
        let resumed = list_due_impl(
            &store,
            &mut state,
            dt(10, 0),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        )
        .expect("succeeds");
        assert!(resumed.due_now.is_some());
    }

    #[test]
    fn an_idle_due_tick_is_discarded_and_logs_no_event_then_re_arms_once_idle_clears() {
        // Given a due rotation tick, but the user is idle (design spec
        // §4.5/§4.7 example E) — an empty chair, unlike a meeting or DND,
        // discards the occurrence rather than deferring it
        let store = store_with_rotation_of_one();
        let mut state = SchedulerState::default();
        let idle = QuietState {
            idle: true,
            ..QuietState::all_clear()
        };

        // When listing due habits at 10:00 while idle
        let held = list_due_impl(&store, &mut state, dt(10, 0), LONDON, idle, None, false)
            .expect("succeeds");

        // Then nothing fires, no rotation state was recorded, and — crucially
        // — no event was logged: a drill the user was never present for
        // leaves no trace in the log
        assert!(held.due_now.is_none());
        assert!(state.rotations.is_empty());
        assert!(store.list_events().expect("list succeeds").is_empty());

        // And when the user comes back at 10:20, nothing greets them — the
        // re-armed tick waits a full interval from their return
        let returned = list_due_impl(
            &store,
            &mut state,
            dt(10, 20),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        )
        .expect("succeeds");
        assert!(returned.due_now.is_none());
        assert_eq!(returned.next_due, Some(dt(10, 50)));

        // And, with the scheduler checking every minute while they're
        // present, it fires once that interval has passed
        for minute in 21..50 {
            let waiting = list_due_impl(
                &store,
                &mut state,
                dt(10, minute),
                LONDON,
                QuietState::all_clear(),
                None,
                false,
            )
            .expect("succeeds");
            assert!(waiting.due_now.is_none());
        }
        let rearmed = list_due_impl(
            &store,
            &mut state,
            dt(10, 50),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        )
        .expect("succeeds");
        assert!(rearmed.due_now.is_some());
    }

    #[test]
    fn a_nudge_still_on_screen_holds_the_next_tick_without_losing_it() {
        // Given a due rotation tick while the previous nudge is still on
        // screen, unresolved
        let store = store_with_rotation_of_one();
        let mut state = SchedulerState::default();

        // When listing due habits
        let held = list_due_impl(
            &store,
            &mut state,
            dt(10, 0),
            LONDON,
            QuietState::all_clear(),
            None,
            true,
        )
        .expect("succeeds");

        // Then nothing new fires and no rotation state is recorded
        assert!(held.due_now.is_none());
        assert!(state.rotations.is_empty());

        // And once the nudge on screen has gone, the held tick can fire
        let freed = list_due_impl(
            &store,
            &mut state,
            dt(10, 1),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        )
        .expect("succeeds");
        assert!(freed.due_now.is_some());
    }

    #[test]
    fn a_freshly_started_scheduler_does_not_nudge_until_a_full_interval_has_passed() {
        // Given the app started (login) at 10:35, inside the day window
        let store = store_with_rotation_of_one();
        let mut state = SchedulerState::starting_at(dt(10, 35));

        // When the first scheduler tick runs a minute later
        let first = list_due_impl(
            &store,
            &mut state,
            dt(10, 36),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        )
        .expect("succeeds");

        // Then nothing fires; the first nudge is a full interval after start
        assert!(first.due_now.is_none());
        assert_eq!(first.next_due, Some(dt(11, 5)));
    }

    #[test]
    fn coming_back_after_the_machine_slept_waits_a_full_interval() {
        // Given a rotation last shown at 12:00, then a check at 12:05 just
        // before the machine was suspended
        let store = store_with_rotation_of_one();
        let mut state = SchedulerState {
            last_checked: Some(dt(11, 59)),
            ..SchedulerState::default()
        };
        let fired = list_due_impl(
            &store,
            &mut state,
            dt(12, 0),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        )
        .expect("succeeds");
        assert!(fired.due_now.is_some());
        list_due_impl(
            &store,
            &mut state,
            dt(12, 5),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        )
        .expect("succeeds");

        // When the next check only arrives at 13:00, after lunch
        let back = list_due_impl(
            &store,
            &mut state,
            dt(13, 0),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        )
        .expect("succeeds");

        // Then the overdue tick does not greet the user on their return
        assert!(back.due_now.is_none());
        assert_eq!(back.next_due, Some(dt(13, 30)));
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
                    fired_at: london(2026, 7, 20, 9, 0),
                    completed: false,
                }),
                ..Default::default()
            },
        );

        // When listing due habits after the next rollover
        list_due_impl(
            &store,
            &mut state,
            dt(5, 0),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        )
        .expect("succeeds");

        // Then an `expired` event was logged
        let events = store.list_events().expect("list succeeds");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].action, EventAction::Expired);

        // And a second call at the same instant does not log it again
        list_due_impl(
            &store,
            &mut state,
            dt(5, 0),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        )
        .expect("succeeds");
        assert_eq!(store.list_events().expect("list succeeds").len(), 1);
    }

    #[test]
    fn a_nudge_still_on_screen_does_not_hold_back_a_fixed_time_habit() {
        // Given a daily 09:00 habit, and a rotation nudge still on screen
        let store = Store::open_in_memory().expect("in-memory store opens");
        store
            .write_config(&default_config())
            .expect("write succeeds");
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
            .expect("insert succeeds");
        let mut state = SchedulerState::default();

        // When its time comes
        let due = list_due_impl(
            &store,
            &mut state,
            dt(9, 0),
            LONDON,
            QuietState::all_clear(),
            None,
            true,
        )
        .expect("succeeds");

        // Then it still fires at its fixed time — only rotation nudges wait
        assert!(due.due_now.is_some());
    }

    #[test]
    fn resolving_a_long_ignored_nudge_moves_the_next_one_a_full_interval_out() {
        // Given a rotation nudge shown at 11:10, then held on screen
        let store = store_with_rotation_of_one();
        let mut state = SchedulerState {
            last_checked: Some(dt(11, 9)),
            ..SchedulerState::default()
        };
        let shown = list_due_impl(
            &store,
            &mut state,
            dt(11, 10),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        )
        .expect("succeeds");
        let habit_id = shown.due_now.expect("fires").habit_id;
        for minute in 11..37 {
            list_due_impl(
                &store,
                &mut state,
                dt(11, minute),
                LONDON,
                QuietState::all_clear(),
                None,
                true,
            )
            .expect("succeeds");
        }

        // When it is finally marked done at 11:37
        super::super::actions::record_action(
            &store,
            &mut state,
            habit_id,
            EventAction::Done,
            dt(11, 37),
            crate::domain::TimeOfDay::new(4, 0).expect("valid time"),
            LONDON,
            Some(dt(11, 10)),
        )
        .expect("succeeds");

        // Then nothing fires at 11:40, when the shown-anchored tick was due,
        // nor any minute until 12:07
        for minute in (37..60).map(|m| dt(11, m)).chain((0..7).map(|m| dt(12, m))) {
            let waiting = list_due_impl(
                &store,
                &mut state,
                minute,
                LONDON,
                QuietState::all_clear(),
                None,
                false,
            )
            .expect("succeeds");
            assert!(waiting.due_now.is_none(), "fired at {minute}");
        }

        // And it fires at 12:07, a full interval after the resolution
        let next = list_due_impl(
            &store,
            &mut state,
            dt(12, 7),
            LONDON,
            QuietState::all_clear(),
            None,
            false,
        )
        .expect("succeeds");
        assert!(next.due_now.is_some());
    }
}
