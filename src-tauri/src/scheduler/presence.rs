//! Rest after return (the nudge cooldown): a rotation never nudges straight
//! after the user arrives — at app start/login, on coming back from being
//! away, or on resolving the previous nudge. Each of those moments sets
//! `SchedulerState::rest_from`, and the next rotation tick waits a full
//! interval from it (see `rotation_due`). This module holds the pure
//! transitions that decide when such a moment happened.

use chrono::{DateTime, Duration, Utc};

use super::state::SchedulerState;

/// A gap this long between two scheduler checks means the machine (or the
/// app) was not running in between — suspend, a closed lid, a lunch break —
/// so the user is treated as just back. Matches the idle probe's threshold:
/// both mean "long enough that this wasn't a moment's pause".
pub const AWAY_GAP_SECS: i64 = 300;

impl SchedulerState {
    /// The state for a scheduler that has just started (app launch, which on
    /// a login-started app is also login): the user has only just arrived,
    /// so the first rotation nudge waits a full interval from `now`.
    pub fn starting_at(now: DateTime<Utc>) -> Self {
        Self {
            rest_from: Some(now),
            last_checked: Some(now),
            ..Self::default()
        }
    }

    /// Records one scheduler check's view of the user's presence. Coming
    /// back from idle, or a check arriving after a long gap since the
    /// previous one, marks `now` as a return and restarts the rest.
    pub fn observe_presence(&mut self, now: DateTime<Utc>, idle: bool) {
        let back_from_idle = self.was_idle && !idle;
        let back_from_gap = self
            .last_checked
            .is_some_and(|last| now - last >= Duration::seconds(AWAY_GAP_SECS));
        if !idle && (back_from_idle || back_from_gap) {
            self.rest_from = Some(now);
        }
        self.was_idle = idle;
        self.last_checked = Some(now);
    }

    /// Records that the user just resolved a nudge (done, skipped or
    /// snoozed): the next rotation nudge waits a full interval from `now`,
    /// however long the previous one sat on screen.
    pub fn rest_after_resolving(&mut self, now: DateTime<Utc>) {
        self.rest_from = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::london;

    fn dt(hour: u32, minute: u32) -> DateTime<Utc> {
        london(2026, 10, 1, hour, minute)
    }

    #[test]
    fn a_freshly_started_scheduler_rests_from_its_start() {
        // When the scheduler starts at 10:35 (app launch / login)
        let state = SchedulerState::starting_at(dt(10, 35));

        // Then the rest starts from that moment
        assert_eq!(state.rest_from, Some(dt(10, 35)));
    }

    #[test]
    fn coming_back_from_idle_restarts_the_rest() {
        // Given a user who was idle at the last check
        let mut state = SchedulerState::starting_at(dt(9, 0));
        state.observe_presence(dt(12, 0), true);

        // When the next check finds them present again
        state.observe_presence(dt(12, 1), false);

        // Then the rest restarts from the moment they came back
        assert_eq!(state.rest_from, Some(dt(12, 1)));
    }

    #[test]
    fn staying_idle_does_not_restart_the_rest() {
        // Given a user already idle
        let mut state = SchedulerState::starting_at(dt(9, 0));
        state.observe_presence(dt(12, 0), true);

        // When they are still idle at the next check
        state.observe_presence(dt(12, 1), true);

        // Then nothing changes yet — the rest starts when they return
        assert_eq!(state.rest_from, Some(dt(9, 0)));
    }

    #[test]
    fn a_long_gap_between_checks_counts_as_coming_back() {
        // Given a check at 12:00, then the machine suspends for lunch
        let mut state = SchedulerState::starting_at(dt(9, 0));
        state.observe_presence(dt(12, 0), false);

        // When the next check only arrives at 13:00
        state.observe_presence(dt(13, 0), false);

        // Then the user is treated as just back
        assert_eq!(state.rest_from, Some(dt(13, 0)));
    }

    #[test]
    fn regular_checks_while_present_leave_the_rest_alone() {
        // Given a user present at every regular minute-ly check
        let mut state = SchedulerState::starting_at(dt(9, 0));
        state.observe_presence(dt(9, 1), false);

        // When the next check arrives a minute later
        state.observe_presence(dt(9, 2), false);

        // Then the rest is unchanged
        assert_eq!(state.rest_from, Some(dt(9, 0)));
    }

    #[test]
    fn a_gap_without_any_earlier_check_is_not_a_return() {
        // Given a state with no record of any earlier check
        let mut state = SchedulerState::default();

        // When it is checked for the first time
        state.observe_presence(dt(10, 0), false);

        // Then no rest is invented — only a real start or return sets one
        assert_eq!(state.rest_from, None);
    }

    #[test]
    fn resolving_a_nudge_restarts_the_rest() {
        // Given a scheduler resting from its start
        let mut state = SchedulerState::starting_at(dt(9, 0));

        // When a long-ignored nudge is finally resolved at 11:37
        state.rest_after_resolving(dt(11, 37));

        // Then the rest restarts from the resolution
        assert_eq!(state.rest_from, Some(dt(11, 37)));
    }
}
