//! At-time and weekly-count due-time logic (design spec §4.2). Weekly-count
//! is implemented as a thin variant of at-time: the same slot logic, gated
//! by an additional per-week completion cap.

use chrono::{Duration, NaiveDate, NaiveDateTime, NaiveTime};

use crate::domain::{
    AtTimeConfig, Habit, Recurrence, ScheduleTrigger, TimeOfDay, Trigger, Weekday,
    WeeklyCountConfig,
};

use super::day::{
    from_chrono_weekday, rollover_day, rollover_day_datetime, to_naive_time, week_start,
};
use super::state::ScheduledHabitState;

/// A scheduled habit's due-now status, when to next check it, and whether an
/// earlier unactioned occurrence has expired.
pub struct ScheduleDue {
    pub due_now: bool,
    pub next_due: NaiveDateTime,
    pub expired: bool,
}

/// Whether `recurrence` includes rollover-day `day`.
fn recurrence_matches(recurrence: &Recurrence, day: NaiveDate) -> bool {
    use chrono::Datelike;
    let weekday = from_chrono_weekday(day.weekday());
    match recurrence {
        Recurrence::Daily => true,
        Recurrence::Weekdays => !matches!(weekday, Weekday::Saturday | Weekday::Sunday),
        Recurrence::SpecificWeekdays { days } => days.contains(&weekday),
    }
}

/// Whether an earlier fire is still outstanding and has rolled over
/// unactioned (design spec §4.2/§4.7 example C) — only applies when
/// `expires_at_day_end` is set.
fn is_expired(
    state: Option<ScheduledHabitState>,
    today: NaiveDate,
    rollover: TimeOfDay,
    expires_at_day_end: bool,
) -> bool {
    expires_at_day_end
        && state
            .and_then(|s| s.last_fire)
            .map(|fire| !fire.completed && rollover_day(fire.fired_at, rollover) < today)
            .unwrap_or(false)
}

/// The next instant, strictly after `after`, whose rollover-day matches
/// `recurrence` and whose slot time is `slot`.
fn next_recurrence_datetime(
    recurrence: &Recurrence,
    after: NaiveDateTime,
    slot: NaiveTime,
    rollover: TimeOfDay,
) -> NaiveDateTime {
    let mut day = rollover_day(after, rollover);
    for _ in 0..14 {
        let candidate = rollover_day_datetime(day, slot, rollover);
        if candidate > after && recurrence_matches(recurrence, day) {
            return candidate;
        }
        day += Duration::days(1);
    }
    unreachable!("every supported recurrence matches at least one day within a fortnight")
}

/// The shared at-time slot logic (design spec §4.2) used by both at-time
/// habits and — capped by a weekly completion count — weekly-count habits.
fn slot_due(
    recurrence: &Recurrence,
    slot_time: TimeOfDay,
    expires_at_day_end: bool,
    now: NaiveDateTime,
    rollover: TimeOfDay,
    state: Option<ScheduledHabitState>,
    is_quiet: bool,
) -> ScheduleDue {
    let today = rollover_day(now, rollover);
    let slot = to_naive_time(slot_time);
    let expired = is_expired(state, today, rollover, expires_at_day_end);

    let fired_today = state
        .and_then(|s| s.last_fire)
        .map(|fire| rollover_day(fire.fired_at, rollover) == today)
        .unwrap_or(false);
    let slot_today = rollover_day_datetime(today, slot, rollover);
    let recurrence_matches_today = recurrence_matches(recurrence, today);

    if !fired_today && recurrence_matches_today && now >= slot_today {
        if is_quiet {
            // Deferred: quiet_state only describes `now`, so the earliest a
            // fresh call could see this fire is `now` itself — the caller is
            // expected to re-poll until a later call finds it clear (design
            // spec §4.5's deferral rule).
            return ScheduleDue {
                due_now: false,
                next_due: now,
                expired,
            };
        }
        return ScheduleDue {
            due_now: true,
            next_due: next_recurrence_datetime(recurrence, now, slot, rollover),
            expired,
        };
    }

    let next_due = if !fired_today && recurrence_matches_today && now < slot_today {
        slot_today
    } else {
        next_recurrence_datetime(recurrence, now, slot, rollover)
    };
    ScheduleDue {
        due_now: false,
        next_due,
        expired,
    }
}

fn at_time_due(
    config: &AtTimeConfig,
    now: NaiveDateTime,
    rollover: TimeOfDay,
    state: Option<ScheduledHabitState>,
    is_quiet: bool,
) -> ScheduleDue {
    slot_due(
        &config.recurrence,
        config.time,
        config.expires_at_day_end,
        now,
        rollover,
        state,
        is_quiet,
    )
}

/// Weekly-count as a thin variant of at-time (design spec §4.2/§4.7 example
/// D): arms daily at its slot until `count` completions are logged this
/// week, then holds until the week resets. If no preferred time is set, the
/// slot auto-chooses the global day window's start.
fn weekly_count_due(
    config: &WeeklyCountConfig,
    now: NaiveDateTime,
    rollover: TimeOfDay,
    day_window_start: TimeOfDay,
    state: Option<ScheduledHabitState>,
    is_quiet: bool,
) -> ScheduleDue {
    let slot_time = config.preferred_time.unwrap_or(day_window_start);
    let today = rollover_day(now, rollover);
    let week = week_start(today);

    let completions_this_week = state
        .filter(|s| s.week_of == Some(week))
        .map(|s| s.weekly_completions)
        .unwrap_or(0);

    if completions_this_week >= config.count {
        let next_week_slot =
            rollover_day_datetime(week + Duration::days(7), to_naive_time(slot_time), rollover);
        return ScheduleDue {
            due_now: false,
            next_due: next_week_slot,
            expired: is_expired(state, today, rollover, config.expires_at_day_end),
        };
    }

    slot_due(
        &Recurrence::Daily,
        slot_time,
        config.expires_at_day_end,
        now,
        rollover,
        state,
        is_quiet,
    )
}

/// Computes due-now status for a schedule-triggered habit, dispatching on
/// its trigger's shape.
pub fn schedule_habit_due(
    habit: &Habit,
    now: NaiveDateTime,
    rollover: TimeOfDay,
    day_window_start: TimeOfDay,
    state: Option<ScheduledHabitState>,
    is_quiet: bool,
) -> ScheduleDue {
    match &habit.trigger {
        Trigger::Schedule(ScheduleTrigger::AtTime(config)) => {
            at_time_due(config, now, rollover, state, is_quiet)
        }
        Trigger::Schedule(ScheduleTrigger::WeeklyCount(config)) => {
            weekly_count_due(config, now, rollover, day_window_start, state, is_quiet)
        }
        Trigger::RotationMember { .. } => {
            unreachable!("ScheduledHabit::new validates the trigger shape")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Weekday as DomainWeekday;
    use crate::scheduler::state::ScheduledFire;

    fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(year, month, day)
            .expect("valid date")
            .and_hms_opt(hour, minute, 0)
            .expect("valid time")
    }

    fn rollover() -> TimeOfDay {
        TimeOfDay::new(4, 0).expect("valid time")
    }

    fn nine_am() -> TimeOfDay {
        TimeOfDay::new(9, 0).expect("valid time")
    }

    fn global_window_start() -> TimeOfDay {
        TimeOfDay::new(9, 0).expect("valid time")
    }

    #[test]
    fn design_spec_example_c_a_daily_scheduled_habit_expires_unactioned_at_rollover() {
        // Given "09:00 daily", expires_at_day_end, fired yesterday and never
        // actioned (design spec §4.7 example C)
        let config = AtTimeConfig {
            time: nine_am(),
            recurrence: Recurrence::Daily,
            expires_at_day_end: true,
        };
        let state = ScheduledHabitState {
            last_fire: Some(ScheduledFire {
                fired_at: dt(2026, 7, 20, 9, 0),
                completed: false,
            }),
            ..Default::default()
        };

        // When checking after the next rollover (04:00 the following day)
        let due = at_time_due(
            &config,
            dt(2026, 7, 21, 5, 0),
            rollover(),
            Some(state),
            false,
        );

        // Then it is reported expired
        assert!(due.expired);

        // And a fresh occurrence is due again at today's 09:00, not
        // suppressed by the expired one
        let due_at_slot = at_time_due(
            &config,
            dt(2026, 7, 21, 9, 0),
            rollover(),
            Some(state),
            false,
        );
        assert!(due_at_slot.due_now);
    }

    #[test]
    fn a_completed_occurrence_does_not_expire() {
        // Given yesterday's occurrence was completed
        let config = AtTimeConfig {
            time: nine_am(),
            recurrence: Recurrence::Daily,
            expires_at_day_end: true,
        };
        let state = ScheduledHabitState {
            last_fire: Some(ScheduledFire {
                fired_at: dt(2026, 7, 20, 9, 0),
                completed: true,
            }),
            ..Default::default()
        };

        // When checking after rollover
        let due = at_time_due(
            &config,
            dt(2026, 7, 21, 5, 0),
            rollover(),
            Some(state),
            false,
        );

        // Then it is not reported expired
        assert!(!due.expired);
    }

    #[test]
    fn a_habit_without_expires_at_day_end_never_expires() {
        // Given an unactioned occurrence, but expires_at_day_end is false
        let config = AtTimeConfig {
            time: nine_am(),
            recurrence: Recurrence::Daily,
            expires_at_day_end: false,
        };
        let state = ScheduledHabitState {
            last_fire: Some(ScheduledFire {
                fired_at: dt(2026, 7, 20, 9, 0),
                completed: false,
            }),
            ..Default::default()
        };

        // When checking well after rollover
        let due = at_time_due(
            &config,
            dt(2026, 7, 21, 5, 0),
            rollover(),
            Some(state),
            false,
        );

        // Then it is never reported expired
        assert!(!due.expired);
    }

    #[test]
    fn a_daily_habit_fires_once_the_slot_time_arrives() {
        // Given "09:00 daily", never fired
        let config = AtTimeConfig {
            time: nine_am(),
            recurrence: Recurrence::Daily,
            expires_at_day_end: true,
        };

        // When checking before the slot
        let before = at_time_due(&config, dt(2026, 7, 21, 8, 59), rollover(), None, false);
        assert!(!before.due_now);
        assert_eq!(before.next_due, dt(2026, 7, 21, 9, 0));

        // Then it fires exactly at the slot
        let at_slot = at_time_due(&config, dt(2026, 7, 21, 9, 0), rollover(), None, false);
        assert!(at_slot.due_now);
    }

    #[test]
    fn a_daily_habit_already_fired_today_does_not_fire_again_today() {
        // Given it already fired at today's slot
        let config = AtTimeConfig {
            time: nine_am(),
            recurrence: Recurrence::Daily,
            expires_at_day_end: true,
        };
        let state = ScheduledHabitState {
            last_fire: Some(ScheduledFire {
                fired_at: dt(2026, 7, 21, 9, 0),
                completed: false,
            }),
            ..Default::default()
        };

        // When checking later the same rollover-day
        let due = at_time_due(
            &config,
            dt(2026, 7, 21, 15, 0),
            rollover(),
            Some(state),
            false,
        );

        // Then it does not fire again, and the next check is tomorrow's slot
        assert!(!due.due_now);
        assert_eq!(due.next_due, dt(2026, 7, 22, 9, 0));
    }

    #[test]
    fn a_weekdays_recurrence_skips_the_weekend() {
        // Given "09:00 weekdays", checked on a Saturday (2026-07-25)
        let config = AtTimeConfig {
            time: nine_am(),
            recurrence: Recurrence::Weekdays,
            expires_at_day_end: false,
        };

        // When checking at the slot time on Saturday
        let due = at_time_due(&config, dt(2026, 7, 25, 9, 0), rollover(), None, false);

        // Then it does not fire, and the next due is Monday
        assert!(!due.due_now);
        assert_eq!(due.next_due, dt(2026, 7, 27, 9, 0));
    }

    #[test]
    fn a_specific_weekdays_recurrence_only_fires_on_the_named_days() {
        // Given "09:00" on Monday and Thursday only
        let config = AtTimeConfig {
            time: nine_am(),
            recurrence: Recurrence::SpecificWeekdays {
                days: vec![DomainWeekday::Monday, DomainWeekday::Thursday],
            },
            expires_at_day_end: false,
        };

        // When checking on a Tuesday
        let due = at_time_due(&config, dt(2026, 7, 21, 9, 0), rollover(), None, false);

        // Then it does not fire, and the next due is Thursday
        assert!(!due.due_now);
        assert_eq!(due.next_due, dt(2026, 7, 23, 9, 0));
    }

    #[test]
    fn a_quiet_slot_time_defers_rather_than_firing() {
        // Given "09:00 daily", checked at the slot time while quiet
        let config = AtTimeConfig {
            time: nine_am(),
            recurrence: Recurrence::Daily,
            expires_at_day_end: false,
        };

        // When checking at 09:00 during a quiet period
        let held = at_time_due(&config, dt(2026, 7, 21, 9, 0), rollover(), None, true);
        assert!(!held.due_now);

        // Then, once quiet clears, it fires
        let cleared = at_time_due(&config, dt(2026, 7, 21, 9, 0), rollover(), None, false);
        assert!(cleared.due_now);
    }

    #[test]
    fn design_spec_example_d_a_weekly_count_habit_arms_daily_until_the_cap_is_reached() {
        // Given "3x / week" with a preferred time of 17:00, no completions
        // logged yet this week (design spec §4.7 example D)
        let config = WeeklyCountConfig {
            count: 3,
            preferred_time: Some(TimeOfDay::new(17, 0).expect("valid time")),
            expires_at_day_end: true,
        };

        // When checking at 17:00 on a day with no prior fire this week
        let due = weekly_count_due(
            &config,
            dt(2026, 7, 21, 17, 0),
            rollover(),
            global_window_start(),
            None,
            false,
        );

        // Then it fires
        assert!(due.due_now);
    }

    #[test]
    fn design_spec_example_d_the_cap_holds_further_instances_until_the_week_resets() {
        // Given 3 completions already logged this week
        let config = WeeklyCountConfig {
            count: 3,
            preferred_time: Some(TimeOfDay::new(17, 0).expect("valid time")),
            expires_at_day_end: true,
        };
        let today = rollover_day(dt(2026, 7, 21, 17, 0), rollover());
        let state = ScheduledHabitState {
            last_fire: None,
            weekly_completions: 3,
            week_of: Some(week_start(today)),
        };

        // When checking at the slot time
        let due = weekly_count_due(
            &config,
            dt(2026, 7, 21, 17, 0),
            rollover(),
            global_window_start(),
            Some(state),
            false,
        );

        // Then it does not arm again, and the next due is next week (the
        // following Monday, since 2026-07-21 falls in the week starting
        // 2026-07-20)
        assert!(!due.due_now);
        assert_eq!(due.next_due, dt(2026, 7, 27, 17, 0));
    }

    #[test]
    fn a_weekly_count_habit_with_a_stale_weekly_completions_count_treats_it_as_reset() {
        // Given 3 completions logged, but for a previous week
        let config = WeeklyCountConfig {
            count: 3,
            preferred_time: Some(TimeOfDay::new(17, 0).expect("valid time")),
            expires_at_day_end: true,
        };
        let state = ScheduledHabitState {
            last_fire: None,
            weekly_completions: 3,
            week_of: Some(week_start(rollover_day(dt(2026, 7, 14, 17, 0), rollover()))),
        };

        // When checking in the new week
        let due = weekly_count_due(
            &config,
            dt(2026, 7, 21, 17, 0),
            rollover(),
            global_window_start(),
            Some(state),
            false,
        );

        // Then it arms again — the reset is implicit in the current week not
        // matching the stale `week_of`
        assert!(due.due_now);
    }

    #[test]
    fn a_weekly_count_habit_without_a_preferred_time_uses_the_global_window_start() {
        // Given a weekly-count habit with no preferred time
        let config = WeeklyCountConfig {
            count: 1,
            preferred_time: None,
            expires_at_day_end: false,
        };

        // When checking at the global day window's start
        let due = weekly_count_due(
            &config,
            dt(2026, 7, 21, 9, 0),
            rollover(),
            global_window_start(),
            None,
            false,
        );

        // Then it fires there, auto-choosing the global window start
        assert!(due.due_now);
    }
}
