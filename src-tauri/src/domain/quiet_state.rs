/// The quiet-rule state as of `now`, injected into the scheduler rather than
/// read from live OS probes (design spec §4.5/§4.6) — idle, an in-progress
/// meeting, Do Not Disturb / Focus, and an in-use microphone each
/// independently gate every trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QuietState {
    /// The user is away (an empty chair) — always-on quiet rule.
    pub idle: bool,
    /// A "real meeting" (per the calendar mode) is happening right now.
    pub in_meeting: bool,
    /// Do Not Disturb / Focus is active.
    pub dnd: bool,
    /// The default input device is capturing — a proxy for an ongoing
    /// (possibly ad-hoc, off-calendar) call.
    pub mic_in_use: bool,
}

impl QuietState {
    /// No quiet source is active — the scheduler may fire freely.
    pub fn all_clear() -> Self {
        Self::default()
    }

    /// Whether any quiet source is currently active, gating a trigger.
    pub fn is_quiet(&self) -> bool {
        self.idle || self.in_meeting || self.dnd || self.mic_in_use
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_clear_has_no_active_quiet_source() {
        // Given the all-clear state
        let state = QuietState::all_clear();

        // Then no quiet source is active
        assert!(!state.is_quiet());
    }

    #[test]
    fn any_single_active_source_makes_the_state_quiet() {
        // Given four states, each with exactly one quiet source active
        let idle = QuietState {
            idle: true,
            ..QuietState::all_clear()
        };
        let in_meeting = QuietState {
            in_meeting: true,
            ..QuietState::all_clear()
        };
        let dnd = QuietState {
            dnd: true,
            ..QuietState::all_clear()
        };
        let mic_in_use = QuietState {
            mic_in_use: true,
            ..QuietState::all_clear()
        };

        // Then each is reported as quiet
        assert!(idle.is_quiet());
        assert!(in_meeting.is_quiet());
        assert!(dnd.is_quiet());
        assert!(mic_in_use.is_quiet());
    }
}
