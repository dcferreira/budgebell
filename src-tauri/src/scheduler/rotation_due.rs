//! Rotation tick due-time logic (design spec §4.3): resolving a rotation's
//! effective window, whether its next tick has arrived, and when to check
//! again.

use chrono::{Duration, NaiveDateTime, NaiveTime};

use crate::domain::{DayConfig, RotationWindow, TimeWindow};

use super::day::to_naive_time;
use super::input::RotationInput;
use super::state::RotationLastShown;

/// The window a rotation is active within, resolved against `day_config`
/// (design spec §4.3) — `None` for an always-on rotation, meaning no time
/// restriction applies.
fn effective_window(window: &RotationWindow, day_config: &DayConfig) -> Option<TimeWindow> {
    match window {
        RotationWindow::Own(window) => Some(*window),
        RotationWindow::InheritGlobal => Some(day_config.day_window),
        RotationWindow::AlwaysOn => None,
    }
}

/// Whether `time` falls inside `window`, handling an overnight window whose
/// start is after its end (domain `TimeWindow` allows start > end).
fn time_in_window(time: NaiveTime, window: TimeWindow) -> bool {
    let start = to_naive_time(window.start);
    let end = to_naive_time(window.end);
    if start <= end {
        time >= start && time < end
    } else {
        time >= start || time < end
    }
}

/// The next instant, at or after `from`, that falls within `window` —
/// `from` itself if `from` is already inside the window's current session.
fn next_window_start_at_or_after(from: NaiveDateTime, window: TimeWindow) -> NaiveDateTime {
    if time_in_window(from.time(), window) {
        return from;
    }
    let start = to_naive_time(window.start);
    let candidate_today = from.date().and_time(start);
    if candidate_today >= from {
        candidate_today
    } else {
        (from.date() + Duration::days(1)).and_time(start)
    }
}

/// A rotation's due-now status and when to next check it.
pub struct RotationDue {
    pub due_now: bool,
    pub next_due: NaiveDateTime,
}

/// Computes whether `rotation`'s next tick is due at `now`, and when to next
/// check it — gating on the rotation's window and on `is_quiet` (design spec
/// §4.5's deferral rule: a tick inside a quiet period holds; the caller
/// re-polls until a later call finds it clear).
pub fn rotation_due(
    rotation: &RotationInput,
    now: NaiveDateTime,
    day_config: &DayConfig,
    last_shown: Option<RotationLastShown>,
    is_quiet: bool,
) -> RotationDue {
    let window = effective_window(&rotation.window, day_config);
    let interval = Duration::seconds(rotation.interval_secs as i64);

    // The tick this rotation would fire at, ignoring quiet gating and window
    // bounds — a fresh rotation's first tick fires as soon as its window is
    // (or becomes) open.
    let candidate = match last_shown {
        Some(shown) => shown.at + interval,
        None => match window {
            Some(w) => next_window_start_at_or_after(now, w),
            None => now,
        },
    };

    let within_window = match window {
        Some(w) => time_in_window(now.time(), w),
        None => true,
    };

    if candidate <= now && within_window && !is_quiet {
        return RotationDue {
            due_now: true,
            next_due: now + interval,
        };
    }

    let next_due = if candidate > now {
        candidate
    } else {
        // Overdue but currently outside the window, or held by a quiet
        // period — the next meaningful check is when the window (re)opens;
        // for an always-on rotation that's immediately (retry on next poll).
        match window {
            Some(w) => next_window_start_at_or_after(now, w),
            None => now,
        }
    };

    RotationDue {
        due_now: false,
        next_due,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Habit, TimeOfDay, Trigger};
    use crate::scheduler::ids::{HabitId, RotationId};
    use crate::scheduler::input::RotationMember;
    use crate::store::Category;

    fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(year, month, day)
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

    fn member(name: &str) -> RotationMember {
        let habit = Habit::new(
            name.to_string(),
            "instructions".to_string(),
            None,
            Category::Exercise,
            true,
            Trigger::rotation_member(1).expect("valid weight"),
        )
        .expect("valid habit");
        RotationMember::new(HabitId(1), habit).expect("valid member")
    }

    fn thirty_minute_rotation(window: RotationWindow) -> RotationInput {
        RotationInput {
            id: RotationId(1),
            interval_secs: 1_800,
            window,
            members: vec![member("Lunge-and-reach")],
        }
    }

    #[test]
    fn design_spec_example_a_a_rotation_tick_inside_a_meeting_defers() {
        // Given a rotation inheriting the global window, last shown at 10:00
        // (design spec §4.7 example A)
        let rotation = thirty_minute_rotation(RotationWindow::InheritGlobal);
        let last_shown = Some(RotationLastShown {
            habit_id: HabitId(1),
            at: dt(2026, 7, 21, 10, 0),
        });

        // When the next tick (10:30) falls during a real meeting
        let due = rotation_due(
            &rotation,
            dt(2026, 7, 21, 10, 30),
            &day_config(),
            last_shown,
            true,
        );

        // Then it does not fire — the tick is held, not lost
        assert!(!due.due_now);

        // And once the meeting has cleared, the tick does fire
        let cleared = rotation_due(
            &rotation,
            dt(2026, 7, 21, 11, 0),
            &day_config(),
            last_shown,
            false,
        );
        assert!(cleared.due_now);
    }

    #[test]
    fn a_tick_due_and_within_window_and_not_quiet_fires_now() {
        // Given a rotation whose interval has elapsed, inside its window
        let rotation = thirty_minute_rotation(RotationWindow::InheritGlobal);
        let last_shown = Some(RotationLastShown {
            habit_id: HabitId(1),
            at: dt(2026, 7, 21, 10, 0),
        });

        // When checking at exactly the next tick
        let due = rotation_due(
            &rotation,
            dt(2026, 7, 21, 10, 30),
            &day_config(),
            last_shown,
            false,
        );

        // Then it fires, and the next tick is one interval later
        assert!(due.due_now);
        assert_eq!(due.next_due, dt(2026, 7, 21, 11, 0));
    }

    #[test]
    fn a_tick_not_yet_reached_is_not_due() {
        // Given a rotation whose interval has not yet elapsed
        let rotation = thirty_minute_rotation(RotationWindow::InheritGlobal);
        let last_shown = Some(RotationLastShown {
            habit_id: HabitId(1),
            at: dt(2026, 7, 21, 10, 0),
        });

        // When checking before the next tick is due
        let due = rotation_due(
            &rotation,
            dt(2026, 7, 21, 10, 15),
            &day_config(),
            last_shown,
            false,
        );

        // Then it does not fire, and the next tick is reported precisely
        assert!(!due.due_now);
        assert_eq!(due.next_due, dt(2026, 7, 21, 10, 30));
    }

    #[test]
    fn a_fresh_rotation_of_one_fires_as_soon_as_its_window_is_open() {
        // Given a rotation of one that has never fired, and `now` already
        // inside its window
        let rotation = thirty_minute_rotation(RotationWindow::InheritGlobal);

        // When checking mid-window
        let due = rotation_due(
            &rotation,
            dt(2026, 7, 21, 11, 0),
            &day_config(),
            None,
            false,
        );

        // Then it fires immediately — a lone interval habit is simply a
        // rotation of one (design spec §4.3)
        assert!(due.due_now);
    }

    #[test]
    fn a_fresh_rotation_before_its_window_opens_waits_for_the_window_start() {
        // Given a rotation that has never fired, checked before its window
        let rotation = thirty_minute_rotation(RotationWindow::InheritGlobal);

        // When checking at 08:00, before the 09:00 global window opens
        let due = rotation_due(&rotation, dt(2026, 7, 21, 8, 0), &day_config(), None, false);

        // Then it does not fire, and the next check is the window's opening
        assert!(!due.due_now);
        assert_eq!(due.next_due, dt(2026, 7, 21, 9, 0));
    }

    #[test]
    fn an_overdue_tick_outside_the_window_waits_for_the_window_to_reopen() {
        // Given a rotation last shown late yesterday, whose window (09-18)
        // has since closed
        let rotation = thirty_minute_rotation(RotationWindow::InheritGlobal);
        let last_shown = Some(RotationLastShown {
            habit_id: HabitId(1),
            at: dt(2026, 7, 20, 17, 45),
        });

        // When checking well outside today's window, e.g. at 20:00
        let due = rotation_due(
            &rotation,
            dt(2026, 7, 21, 20, 0),
            &day_config(),
            last_shown,
            false,
        );

        // Then it does not fire, and the next check is tomorrow's window open
        assert!(!due.due_now);
        assert_eq!(due.next_due, dt(2026, 7, 22, 9, 0));
    }

    #[test]
    fn an_overdue_tick_fires_as_soon_as_the_window_reopens() {
        // Given the same overdue rotation as above
        let rotation = thirty_minute_rotation(RotationWindow::InheritGlobal);
        let last_shown = Some(RotationLastShown {
            habit_id: HabitId(1),
            at: dt(2026, 7, 20, 17, 45),
        });

        // When checking right as tomorrow's window opens
        let due = rotation_due(
            &rotation,
            dt(2026, 7, 22, 9, 0),
            &day_config(),
            last_shown,
            false,
        );

        // Then it fires immediately — overdue ticks aren't lost, they carry
        // over to the next window session
        assert!(due.due_now);
    }

    #[test]
    fn an_always_on_rotation_ignores_window_bounds() {
        // Given an always-on rotation, checked in the middle of the night
        let rotation = thirty_minute_rotation(RotationWindow::AlwaysOn);

        // When checking at 02:00 with no prior tick
        let due = rotation_due(&rotation, dt(2026, 7, 21, 2, 0), &day_config(), None, false);

        // Then it fires — always-on means no time restriction applies
        assert!(due.due_now);
    }

    #[test]
    fn an_own_window_rotation_uses_its_own_bounds_not_the_global_window() {
        // Given a rotation with its own early-morning window, outside the
        // global 09:00-18:00 window
        let own_window = TimeWindow::new(
            TimeOfDay::new(6, 0).expect("valid time"),
            TimeOfDay::new(8, 0).expect("valid time"),
        )
        .expect("valid window");
        let rotation = thirty_minute_rotation(RotationWindow::Own(own_window));

        // When checking at 07:00 — inside its own window, but before the
        // global window opens
        let due = rotation_due(&rotation, dt(2026, 7, 21, 7, 0), &day_config(), None, false);

        // Then it fires — the rotation's own window governs, not the global one
        assert!(due.due_now);
    }

    #[test]
    fn idle_holds_a_due_tick_and_it_re_arms_once_idle_clears() {
        // Given a rotation whose tick is due (design spec §4.7 example E)
        let rotation = thirty_minute_rotation(RotationWindow::InheritGlobal);
        let last_shown = Some(RotationLastShown {
            habit_id: HabitId(1),
            at: dt(2026, 7, 21, 13, 30),
        });

        // When the user is idle at the moment the tick is due (14:00)
        let held = rotation_due(
            &rotation,
            dt(2026, 7, 21, 14, 0),
            &day_config(),
            last_shown,
            true,
        );
        assert!(!held.due_now);

        // Then, once idle clears, the held tick fires
        let rearmed = rotation_due(
            &rotation,
            dt(2026, 7, 21, 14, 0),
            &day_config(),
            last_shown,
            false,
        );
        assert!(rearmed.due_now);
    }
}
