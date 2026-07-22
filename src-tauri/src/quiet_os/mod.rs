//! OS quiet probes that produce a `QuietState` for the pure scheduler
//! (design spec §8/§9.2). Each of the three always-on quiet sources — idle,
//! Do Not Disturb / Focus, and an in-progress calendar meeting — is read from
//! the OS behind a thin wrapper here. The scheduler never calls these; it only
//! ever consumes the `QuietState` they produce, keeping all scheduling logic
//! pure and deterministic.
//!
//! Each probe is gated by its config toggle (design spec §3.6): a disabled
//! source is reported as clear rather than being probed at all. Everything is
//! 100% local — `ioreg`, a per-user Focus assertions file, an ad-hoc-signed
//! EventKit helper, and an ad-hoc-signed CoreAudio helper. No network I/O.

mod calendar;
mod dnd;
mod error;
mod idle;
mod mic;

pub use calendar::{classify_real_meeting_now, parse_events, CalendarEvent};
pub use dnd::parse_focus_active;
pub use error::QuietOsError;
pub use idle::{is_idle, parse_hid_idle_seconds, IDLE_THRESHOLD_SECS};
pub use mic::parse_mic_running;

use chrono::NaiveDateTime;

use crate::domain::QuietState;
use crate::store::Config;

/// Reads the OS quiet sources enabled in `config` and assembles the
/// `QuietState` as of `now`. A source whose toggle is off is not probed and
/// reported as clear. Any probe that cannot answer fails loudly.
pub fn probe_quiet_state(
    config: &Config,
    now: NaiveDateTime,
) -> Result<QuietState, QuietOsError> {
    let idle = if config.idle_enabled {
        idle::probe_idle()?
    } else {
        false
    };
    let dnd = if config.dnd_enabled {
        dnd::probe_dnd()?
    } else {
        false
    };
    let in_meeting = if config.calendar_pause_enabled {
        calendar::probe_real_meeting_now(now, config.calendar_mode)?
    } else {
        false
    };
    let mic_in_use = if config.mic_pause_enabled {
        mic::probe_mic_in_use()?
    } else {
        false
    };

    Ok(QuietState {
        idle,
        in_meeting,
        dnd,
        mic_in_use,
    })
}
