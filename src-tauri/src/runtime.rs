//! The runtime bridge (design spec §10 "seed-wire"): a background scheduler
//! tick that, when a habit falls due, surfaces it in the corner toast window.
//!
//! Everything here is impure glue around already-tested pieces — the pure
//! scheduler, the `list_due` command edge, and the store. The one part that
//! carries real design intent, the toast window's flags (always-on-top,
//! frameless, never-focus-stealing), is factored into the pure
//! [`toast_window_spec`] so it can be asserted without a running app.

use std::time::Duration;

use tauri::{
    AppHandle, Emitter, LogicalPosition, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use thiserror::Error;

use crate::commands::{list_due_now, AppState, CommandError, CurrentDue, DueHabitDto};
use crate::media::resolve_media;
use crate::quiet_os::{probe_quiet_state, QuietOsError};
use crate::store::Habit;

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
// Tall enough that the reworked card — an 88px media thumbnail beside a
// two-line instruction clamp (design spec §5) — fits without the webview
// growing a scrollbar. The window is transparent and the card is pinned to
// the top, so the extra height below it is invisible.
const TOAST_HEIGHT: f64 = 230.0;

/// Inset (in logical pixels) from the primary monitor's top-right corner at
/// which the floating toast card is pinned.
const TOAST_INSET: f64 = 20.0;

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

/// Withdraws the toast — destroying the window and discarding the current due
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
    // Destroy (not hide) the toast window so the transparent OS layer — and its
    // drop-shadow — leaves nothing lingering on screen. `ensure_toast_window`
    // rebuilds a fresh one on the next due habit.
    if let Some(window) = app.get_webview_window(TOAST_LABEL) {
        window.destroy()?;
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
    let due = with_resolved_media(app, due)?;
    let shown_at = chrono::Local::now().naive_local();
    app.state::<AppState>().lock()?.current_due = Some(CurrentDue {
        due: due.clone(),
        shown_at,
    });
    ensure_toast_window(app)?;
    app.emit_to(TOAST_LABEL, HABIT_DUE_EVENT, due)?;
    Ok(())
}

/// Maps the due habit's `media_path` — a relative filename under the media
/// directory, or `None` — through [`resolve_media`] so every read path that
/// flows through `present_toast` (the stored `current_due`, the `habit-due`
/// event, and the `current_due` command that returns the former) carries an
/// absolute, webview-loadable path instead of the DB's relative one (design
/// spec §4.1). A `None` media_path stays `None`.
fn with_resolved_media(app: &AppHandle, mut due: DueHabitDto) -> Result<DueHabitDto, RuntimeError> {
    let media_dir = app.path().app_data_dir()?.join("media");
    due.media_path = due
        .media_path
        .as_deref()
        .and_then(|relative| resolve_media(&media_dir, relative))
        .map(|path| path.to_string_lossy().into_owned());
    Ok(due)
}

/// Shows the toast window, creating it with the design-spec flags on first use.
/// It is deliberately shown *without* focus so it never interrupts typing, and
/// is transparent so only the floating card — not a window chrome — is seen.
fn ensure_toast_window(app: &AppHandle) -> Result<(), RuntimeError> {
    if let Some(window) = app.get_webview_window(TOAST_LABEL) {
        window.show()?;
        return Ok(());
    }
    let spec = toast_window_spec();
    let window = WebviewWindowBuilder::new(app, spec.label, WebviewUrl::App(spec.url.into()))
        .title("habits")
        .inner_size(spec.width, spec.height)
        .resizable(spec.resizable)
        .decorations(spec.decorations)
        .always_on_top(spec.always_on_top)
        .focused(spec.focused)
        .skip_taskbar(spec.skip_taskbar)
        .transparent(true)
        // No OS window shadow: a transparent window otherwise paints a shadow
        // rectangle around the card (and leaves a ghost box behind on close).
        // The Toast card supplies its own CSS shadow instead.
        .shadow(false)
        // Krisp-style: the toast floats over every Space and full-screen app,
        // not just the current desktop, so a due nudge is never hidden behind
        // whatever the user has focused.
        .visible_on_all_workspaces(true)
        .build()?;
    position_top_right(&window, spec.width)?;
    Ok(())
}

/// Pins the toast card to the top-right of the primary monitor, inset by
/// [`TOAST_INSET`]. Computed in logical coordinates so it lands correctly on
/// Retina/scaled displays. A no-op if no primary monitor is reported.
fn position_top_right(window: &WebviewWindow, toast_width: f64) -> Result<(), RuntimeError> {
    let Some(monitor) = window.primary_monitor()? else {
        return Ok(());
    };
    let scale = monitor.scale_factor();
    let monitor_logical_width = monitor.size().width as f64 / scale;
    let origin_x = monitor.position().x as f64 / scale;
    let origin_y = monitor.position().y as f64 / scale;
    let x = origin_x + monitor_logical_width - toast_width - TOAST_INSET;
    let y = origin_y + TOAST_INSET;
    window.set_position(LogicalPosition::new(x, y))?;
    Ok(())
}

/// Picks a habit to surface for the tray's "Do a drill now" (design spec
/// §3.3.1) when nothing is scheduled-due. Prefers a rotation member (the
/// movement snacks) over a one-off scheduled habit, and considers only enabled
/// habits — yielding `None` when nothing is enabled rather than inventing one.
pub fn pick_drill_now(habits: &[Habit]) -> Option<&Habit> {
    habits
        .iter()
        .filter(|habit| habit.enabled)
        .find(|habit| habit.rotation_id.is_some())
        .or_else(|| habits.iter().find(|habit| habit.enabled))
}

/// Flattens a store habit row into the IPC-shaped due-habit the toast renders.
fn due_from_habit(habit: &Habit) -> DueHabitDto {
    DueHabitDto {
        habit_id: habit.id,
        name: habit.name.clone(),
        instructions: habit.instructions.clone(),
        media_path: habit.media_path.clone(),
        category: habit.category,
    }
}

/// Surfaces a drill immediately for the tray's "Do a drill now" (design spec
/// §3.3.1), reusing the exact toast-present path a scheduler tick uses. It
/// prefers whatever the scheduler considers due right now so the on-demand
/// drill is consistent with the normal flow; only if nothing is due does it
/// fall back to picking an enabled habit directly, so the user always gets a
/// drill on demand.
pub fn drill_now(app: &AppHandle) -> Result<(), RuntimeError> {
    if let Some(due) = list_due_now(&app.state::<AppState>())?.due_now {
        return present_toast(app, due);
    }
    let habits = {
        let state = app.state::<AppState>();
        let guard = state.lock()?;
        guard.store.list_habits().map_err(CommandError::from)?
    };
    let Some(due) = pick_drill_now(&habits).map(due_from_habit) else {
        return Ok(());
    };
    present_toast(app, due)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{Category, TriggerKind};

    /// A minimal enabled habit row, tweakable per test.
    fn habit(id: i64, name: &str, enabled: bool, rotation_id: Option<i64>) -> Habit {
        Habit {
            id,
            name: name.to_string(),
            instructions: "do the thing".to_string(),
            media_path: None,
            category: Category::Exercise,
            enabled,
            trigger_kind: if rotation_id.is_some() {
                TriggerKind::RotationMember
            } else {
                TriggerKind::ScheduleAtTime
            },
            trigger_config_json: "{}".to_string(),
            weight: rotation_id.map(|_| 1),
            rotation_id,
            created_at: 0,
        }
    }

    #[test]
    fn drill_now_prefers_a_rotation_member_over_a_scheduled_habit() {
        // Given both a scheduled habit and a rotation member, all enabled
        let habits = vec![
            habit(1, "Morning stretch", true, None),
            habit(2, "Lunge-and-reach", true, Some(10)),
        ];

        // When picking a drill to show on demand
        // Then the rotation member (a movement snack) is preferred
        assert_eq!(
            pick_drill_now(&habits).map(|h| h.id),
            Some(2),
            "a rotation member should win over a one-off scheduled habit"
        );
    }

    #[test]
    fn drill_now_falls_back_to_any_enabled_habit_when_no_rotation_member_exists() {
        // Given only scheduled habits, none in a rotation
        let habits = vec![
            habit(1, "Morning stretch", true, None),
            habit(2, "Evening walk", true, None),
        ];

        // When picking a drill to show on demand
        // Then the first enabled habit is used rather than nothing
        assert_eq!(pick_drill_now(&habits).map(|h| h.id), Some(1));
    }

    #[test]
    fn drill_now_skips_disabled_habits() {
        // Given a disabled rotation member ahead of an enabled scheduled habit
        let habits = vec![
            habit(1, "Retired drill", false, Some(10)),
            habit(2, "Morning stretch", true, None),
        ];

        // When picking a drill to show on demand
        // Then the disabled member is skipped in favour of the enabled habit
        assert_eq!(pick_drill_now(&habits).map(|h| h.id), Some(2));
    }

    #[test]
    fn drill_now_yields_nothing_when_no_habit_is_enabled() {
        // Given no enabled habits at all
        let habits = vec![habit(1, "Retired drill", false, Some(10))];

        // When picking a drill to show on demand
        // Then nothing is surfaced rather than inventing a disabled one
        assert!(pick_drill_now(&habits).is_none());
    }

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
