//! The runtime bridge (design spec §10 "seed-wire"): a background scheduler
//! tick that, when a habit falls due, surfaces it in the corner toast window.
//!
//! Everything here is impure glue around already-tested pieces — the pure
//! scheduler, the `list_due` command edge, and the store. The one part that
//! carries real design intent, the toast window's flags (always-on-top,
//! frameless, never-focus-stealing), is factored into the pure
//! [`toast_window_spec`] so it can be asserted without a running app.

use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use thiserror::Error;

use crate::commands::{list_due_now, AppState, CommandError, CurrentDue, DueHabitDto};
use crate::quiet_os::{probe_quiet_state, QuietOsError};

/// How often the scheduler tick wakes to check what is due.
pub const TICK_INTERVAL_SECS: u64 = 60;

/// The toast window's stable label — reused so repeated ticks reshow the one
/// window rather than spawning duplicates.
pub const TOAST_LABEL: &str = "toast";

/// The frontend route the toast window loads.
pub const TOAST_URL: &str = "index.html?view=toast";

/// The event the tick emits to the toast window carrying the due habit.
pub const HABIT_DUE_EVENT: &str = "habit-due";

const TOAST_WIDTH: f64 = 360.0;
const TOAST_HEIGHT: f64 = 200.0;

/// Errors raised inside the tick. Logged rather than fatal — a transient
/// failure must not take the app down.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error(transparent)]
    Command(#[from] CommandError),

    #[error(transparent)]
    Tauri(#[from] tauri::Error),

    #[error(transparent)]
    QuietOs(#[from] QuietOsError),
}

/// The toast window's configuration (design spec §3.1). Factored out as a pure
/// value so its load-bearing flags can be unit-tested without a Tauri app.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToastWindowSpec {
    pub label: &'static str,
    pub url: &'static str,
    /// Pinned above other windows.
    pub always_on_top: bool,
    /// Frameless — no title bar or borders.
    pub decorations: bool,
    /// Never steals keyboard focus.
    pub focused: bool,
    /// Kept out of the taskbar/Dock — a transient nudge, not an app window.
    pub skip_taskbar: bool,
    pub resizable: bool,
    pub width: f64,
    pub height: f64,
}

/// The corner toast's window spec (design spec §3.1): always-on-top,
/// frameless, and never focus-stealing.
pub fn toast_window_spec() -> ToastWindowSpec {
    ToastWindowSpec {
        label: TOAST_LABEL,
        url: TOAST_URL,
        always_on_top: true,
        decorations: false,
        focused: false,
        skip_taskbar: true,
        resizable: false,
        width: TOAST_WIDTH,
        height: TOAST_HEIGHT,
    }
}

/// Spawns the background scheduler tick on a plain OS thread (no async runtime
/// needed). It sleeps between checks and surfaces the toast whenever a habit is
/// due. Failures are logged, never fatal.
pub fn spawn_scheduler_tick(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(TICK_INTERVAL_SECS));
        if let Err(error) = tick_once(&app) {
            eprintln!("the scheduler tick failed: {error}");
        }
    });
}

/// One scheduler check: first withdraw a standing toast if the user has gone
/// idle since it was shown (design spec §4.5's "goes idle mid-drill" case,
/// example F), then compute what is newly due (applying every quiet rule and
/// state transition via the shared `list_due` edge — which already holds an
/// idle *tick* rather than showing anything, design spec §4.7 example E) and,
/// if something is, present it in the toast.
fn tick_once(app: &AppHandle) -> Result<(), RuntimeError> {
    withdraw_toast_if_idle(app)?;
    let decision = list_due_now(&app.state::<AppState>())?;
    let Some(due) = decision.due_now else {
        return Ok(());
    };
    present_toast(app, due)
}

/// Whether a currently-shown toast should be withdrawn and its occurrence
/// discarded (design spec §4.5): a standing toast is never left on screen
/// while the user is idle, because the elapsed time would no longer reflect
/// the user being present — nothing is logged when this fires.
fn should_withdraw_toast(toast_currently_shown: bool, idle: bool) -> bool {
    toast_currently_shown && idle
}

/// Withdraws the toast — hiding the window and discarding the current due
/// occurrence with no event logged — if it is showing and the user has since
/// gone idle. A no-op whenever no toast is currently up.
fn withdraw_toast_if_idle(app: &AppHandle) -> Result<(), RuntimeError> {
    let state = app.state::<AppState>();
    let toast_currently_shown = state.lock()?.current_due.is_some();
    if !toast_currently_shown {
        return Ok(());
    }

    let idle = current_idle_state(app)?;
    if !should_withdraw_toast(toast_currently_shown, idle) {
        return Ok(());
    }

    state.lock()?.current_due = None;
    if let Some(window) = app.get_webview_window(TOAST_LABEL) {
        window.hide()?;
    }
    Ok(())
}

/// Reads the live idle probe, gated by its config toggle, exactly as the
/// `list_due` edge does — kept separate here so the idle-withdraw check
/// above runs even on a tick where nothing new becomes due.
fn current_idle_state(app: &AppHandle) -> Result<bool, RuntimeError> {
    let state = app.state::<AppState>();
    let inner = state.lock()?;
    let config = inner
        .store
        .read_config()
        .map_err(CommandError::from)?
        .ok_or(CommandError::ConfigNotSet)?;
    drop(inner);
    Ok(probe_quiet_state(&config, chrono::Local::now().naive_local())?.idle)
}

/// Records the due habit as the current nudge — including the instant its
/// toast was shown (design spec §3.8/§4.5), so `complete_habit`/`skip_habit`
/// can later compute a duration — surfaces the toast window, then pushes the
/// habit to it.
fn present_toast(app: &AppHandle, due: DueHabitDto) -> Result<(), RuntimeError> {
    let shown_at = chrono::Local::now().naive_local();
    app.state::<AppState>().lock()?.current_due = Some(CurrentDue {
        due: due.clone(),
        shown_at,
    });
    ensure_toast_window(app)?;
    app.emit_to(TOAST_LABEL, HABIT_DUE_EVENT, due)?;
    Ok(())
}

/// Shows the toast window, creating it with the design-spec flags on first use.
/// It is deliberately shown *without* focus so it never interrupts typing.
fn ensure_toast_window(app: &AppHandle) -> Result<(), RuntimeError> {
    if let Some(window) = app.get_webview_window(TOAST_LABEL) {
        window.show()?;
        return Ok(());
    }
    let spec = toast_window_spec();
    WebviewWindowBuilder::new(app, spec.label, WebviewUrl::App(spec.url.into()))
        .title("habits")
        .inner_size(spec.width, spec.height)
        .resizable(spec.resizable)
        .decorations(spec.decorations)
        .always_on_top(spec.always_on_top)
        .focused(spec.focused)
        .skip_taskbar(spec.skip_taskbar)
        .build()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_toast_window_is_always_on_top_frameless_and_never_focus_stealing() {
        // Given the toast window spec (design spec §3.1)
        let spec = toast_window_spec();

        // Then it is pinned on top, frameless, and never takes focus
        assert!(spec.always_on_top);
        assert!(!spec.decorations, "frameless => decorations off");
        assert!(!spec.focused, "must never steal keyboard focus");
        assert!(spec.skip_taskbar);
    }

    #[test]
    fn the_toast_window_loads_the_toast_route_under_a_stable_label() {
        // Given the toast window spec
        let spec = toast_window_spec();

        // Then it loads the dedicated toast view under the reused label so
        // repeat ticks reshow one window rather than spawning duplicates
        assert_eq!(spec.label, TOAST_LABEL);
        assert_eq!(spec.url, "index.html?view=toast");
    }

    #[test]
    fn a_standing_toast_is_withdrawn_once_the_user_goes_idle() {
        // Given a toast currently showing (design spec §4.5/§4.7 example F)
        // When the user has since gone idle
        // Then it should be withdrawn
        assert!(should_withdraw_toast(true, true));
    }

    #[test]
    fn a_standing_toast_is_left_alone_while_the_user_is_present() {
        // Given a toast currently showing
        // When the user is not idle
        // Then it should not be withdrawn
        assert!(!should_withdraw_toast(true, false));
    }

    #[test]
    fn no_toast_showing_means_nothing_to_withdraw_even_if_idle() {
        // Given no toast currently showing
        // When the user happens to be idle
        // Then there is nothing to withdraw — idle-at-due-time is the pure
        // scheduler's job (design spec §4.7 example E), not this check's
        assert!(!should_withdraw_toast(false, true));
    }
}
