//! The pure scheduler engine (design spec §4.6) — the heart of the app. A
//! single free function, `schedule`, decides which habit (if any) is due
//! right now, when to next check, and which scheduled habits have expired
//! since the last rollover.
//!
//! It reads nothing from the wall clock, filesystem, RNG, or calendar
//! directly: `now`, `quiet_state`, and the picker's `rng_seed` are all
//! injected, so the same inputs always produce the same `Decision`. State
//! transitions — recording what fired, marking done/skip/expired, resetting
//! weekly counts — are the caller's job, applied *after* reading a
//! `Decision`, never inside this module.

mod day;
mod decision;
mod error;
mod ids;
mod input;
mod picker;
mod rotation_due;
mod schedule_due;
mod state;

pub use day::{rollover_day, week_start};
pub use decision::{Decision, DueHabit, Expiration};
pub use error::SchedulerError;
pub use ids::{HabitId, RotationId};
pub use input::{RotationInput, RotationMember, ScheduledHabit};
pub use state::{RotationLastShown, ScheduledFire, ScheduledHabitState, SchedulerState};

use crate::domain::{DayConfig, QuietState};

/// Decides which habit is due, right now, given everything the scheduler is
/// allowed to know (design spec §4.6's pure-scheduler contract).
///
/// `rng_seed` drives the rotation picker's weighted draw deterministically —
/// the caller supplies it (e.g. derived from a persisted counter), so the
/// same seed and state always yield the same pick.
pub fn schedule(
    scheduled_habits: &[ScheduledHabit],
    rotations: &[RotationInput],
    now: chrono::NaiveDateTime,
    quiet_state: QuietState,
    day_config: DayConfig,
    state: &SchedulerState,
    rng_seed: u64,
) -> Decision {
    let is_quiet = quiet_state.is_quiet();
    let mut expirations = Vec::new();
    let mut next_due_candidates = Vec::new();
    let mut due_now = None;

    let mut sorted_scheduled: Vec<&ScheduledHabit> = scheduled_habits.iter().collect();
    sorted_scheduled.sort_by_key(|habit| habit.id);
    for entry in sorted_scheduled {
        let habit_state = state.scheduled_habits.get(&entry.id).copied();
        let due = schedule_due::schedule_habit_due(
            &entry.habit,
            now,
            day_config.rollover,
            day_config.day_window.start,
            habit_state,
            is_quiet,
        );
        if due.expired {
            expirations.push(Expiration {
                habit_id: entry.id,
                habit: entry.habit.clone(),
            });
        }
        next_due_candidates.push(due.next_due);
        if due.due_now && due_now.is_none() {
            due_now = Some(DueHabit {
                habit_id: entry.id,
                habit: entry.habit.clone(),
            });
        }
    }

    // Rotations are only consulted for `due_now` once no scheduled habit has
    // already claimed this call's single slot — a deterministic tie-break
    // for the (rare) case two triggers land on the same instant.
    let mut sorted_rotations: Vec<&RotationInput> = rotations.iter().collect();
    sorted_rotations.sort_by_key(|rotation| rotation.id);
    for rotation in sorted_rotations {
        let last_shown = state.rotations.get(&rotation.id).copied();
        let due = rotation_due::rotation_due(rotation, now, &day_config, last_shown, is_quiet);
        next_due_candidates.push(due.next_due);
        if due.due_now && due_now.is_none() {
            let previous = last_shown.map(|shown| shown.habit_id);
            let candidates: Vec<(HabitId, u32)> = rotation
                .members
                .iter()
                .map(|member| (member.habit_id, member.weight()))
                .collect();
            let picked_id = picker::pick(&candidates, previous, rng_seed);
            let picked_habit = rotation
                .members
                .iter()
                .find(|member| member.habit_id == picked_id)
                .expect("pick always returns an id from the candidates it was given")
                .habit
                .clone();
            due_now = Some(DueHabit {
                habit_id: picked_id,
                habit: picked_habit,
            });
        }
    }

    Decision {
        due_now,
        next_due: next_due_candidates.into_iter().min(),
        expirations,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use chrono::NaiveDate;

    use crate::domain::{Habit, Recurrence, RotationWindow, TimeOfDay, TimeWindow, Trigger};
    use crate::store::Category;

    use super::*;

    fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .expect("valid date")
            .and_hms_opt(hour, minute, 0)
            .expect("valid time")
    }

    fn day_config() -> DayConfig {
        DayConfig {
            rollover: TimeOfDay::new(4, 0).expect("valid time"),
            day_window: TimeWindow::new(
                TimeOfDay::new(9, 0).expect("valid time"),
                TimeOfDay::new(18, 0).expect("valid time"),
            )
            .expect("valid window"),
        }
    }

    fn rotation_member_habit(name: &str, weight: u32) -> Habit {
        Habit::new(
            name.to_string(),
            "instructions".to_string(),
            None,
            Category::Exercise,
            true,
            Trigger::rotation_member(weight).expect("valid weight"),
        )
        .expect("valid habit")
    }

    /// Design spec §4.7 example B's rotation.
    fn example_b_rotation() -> RotationInput {
        RotationInput {
            id: RotationId(1),
            interval_secs: 1_800,
            window: RotationWindow::InheritGlobal,
            members: vec![
                RotationMember::new(HabitId(1), rotation_member_habit("Lunge-and-reach", 2))
                    .expect("valid member"),
                RotationMember::new(HabitId(2), rotation_member_habit("Glute bridges", 1))
                    .expect("valid member"),
                RotationMember::new(HabitId(3), rotation_member_habit("Wall sit->squat", 1))
                    .expect("valid member"),
            ],
        }
    }

    #[test]
    fn design_spec_example_b_the_picker_never_repeats_the_immediately_previous_member() {
        // Given the rotation from example B, last shown Lunge-and-reach
        let rotation = example_b_rotation();
        let mut state = SchedulerState::default();
        state.rotations.insert(
            RotationId(1),
            RotationLastShown {
                habit_id: HabitId(1),
                at: dt(2026, 7, 21, 10, 0),
            },
        );

        // When the next tick fires at 10:30
        let decision = schedule(
            &[],
            &[rotation],
            dt(2026, 7, 21, 10, 30),
            QuietState::all_clear(),
            day_config(),
            &state,
            0,
        );

        // Then a habit fires, and it is never the immediately-previous one
        let due = decision.due_now.expect("a tick is due");
        assert_ne!(due.habit_id, HabitId(1));
    }

    #[test]
    fn a_lone_rotation_habit_fires_with_no_last_shown_state() {
        // Given a rotation of one, never shown
        let rotation = RotationInput {
            id: RotationId(1),
            interval_secs: 1_800,
            window: RotationWindow::AlwaysOn,
            members: vec![RotationMember::new(
                HabitId(1),
                rotation_member_habit("Lunge-and-reach", 1),
            )
            .expect("valid member")],
        };

        // When checking with an empty state
        let decision = schedule(
            &[],
            &[rotation],
            dt(2026, 7, 21, 10, 0),
            QuietState::all_clear(),
            day_config(),
            &SchedulerState::default(),
            0,
        );

        // Then it fires
        assert_eq!(
            decision.due_now.expect("a tick is due").habit_id,
            HabitId(1)
        );
    }

    #[test]
    fn a_quiet_state_holds_every_trigger_and_reports_nothing_due() {
        // Given a rotation and a scheduled habit both due at the same instant
        let rotation = RotationInput {
            id: RotationId(1),
            interval_secs: 1_800,
            window: RotationWindow::AlwaysOn,
            members: vec![RotationMember::new(
                HabitId(1),
                rotation_member_habit("Lunge-and-reach", 1),
            )
            .expect("valid member")],
        };
        let scheduled = ScheduledHabit::new(
            HabitId(2),
            Habit::new(
                "Morning stretch".to_string(),
                "instructions".to_string(),
                None,
                Category::General,
                true,
                Trigger::at_time(
                    TimeOfDay::new(9, 0).expect("valid time"),
                    Recurrence::Daily,
                    false,
                )
                .expect("valid trigger"),
            )
            .expect("valid habit"),
        )
        .expect("valid scheduled habit");

        // When the shared quiet state is active (idle)
        let mut quiet = QuietState::all_clear();
        quiet.idle = true;
        let decision = schedule(
            &[scheduled],
            &[rotation],
            dt(2026, 7, 21, 9, 0),
            quiet,
            day_config(),
            &SchedulerState::default(),
            0,
        );

        // Then nothing fires — quiet gates every trigger (design spec §4.5)
        assert!(decision.due_now.is_none());
    }

    #[test]
    fn design_spec_example_c_an_expired_scheduled_habit_is_reported_and_a_fresh_slot_still_arms() {
        // Given a daily 09:00 habit that fired yesterday and was never
        // actioned (design spec §4.7 example C)
        let habit = Habit::new(
            "Morning stretch".to_string(),
            "instructions".to_string(),
            None,
            Category::General,
            true,
            Trigger::at_time(
                TimeOfDay::new(9, 0).expect("valid time"),
                Recurrence::Daily,
                true,
            )
            .expect("valid trigger"),
        )
        .expect("valid habit");
        let scheduled = ScheduledHabit::new(HabitId(1), habit).expect("valid scheduled habit");

        let mut scheduled_habits = HashMap::new();
        scheduled_habits.insert(
            HabitId(1),
            ScheduledHabitState {
                last_fire: Some(ScheduledFire {
                    fired_at: dt(2026, 7, 20, 9, 0),
                    completed: false,
                }),
                ..Default::default()
            },
        );
        let state = SchedulerState {
            rotations: HashMap::new(),
            scheduled_habits,
        };

        // When checking after the rollover, at today's slot
        let decision = schedule(
            &[scheduled],
            &[],
            dt(2026, 7, 21, 9, 0),
            QuietState::all_clear(),
            day_config(),
            &state,
            0,
        );

        // Then yesterday's occurrence is reported expired, and today's fresh
        // occurrence still fires
        assert_eq!(decision.expirations.len(), 1);
        assert_eq!(decision.expirations[0].habit_id, HabitId(1));
        assert_eq!(
            decision.due_now.expect("a fresh slot fires").habit_id,
            HabitId(1)
        );
    }
}
