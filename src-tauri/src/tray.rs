//! The macOS menu-bar (tray) icon and its menu (design spec §3.3).
//!
//! Menu items, in order:
//!   1. Do a drill now
//!   2. Pause nudges  → submenu: 30 min / 1 hour / Custom…
//!   3. Resume nudges
//!   4. Today's stats
//!   5. Settings…
//!   6. Quit
//!
//! The heart of this module is the *pure* [`TrayAction::from_menu_id`]
//! mapping — it has no dependency on a running Tauri app, so it is
//! exhaustively unit-tested. Everything that actually touches the app
//! (building the menu, opening windows, mutating pause state) is a thin
//! shell around that mapping and is exercised by live-app verification.

use chrono::{Duration, Local};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::commands::AppState;

/// Monochrome menu-bar glyph, bundled at compile time. Rendered as a macOS
/// *template* image (see [`setup_tray`]) so AppKit tints it to match the menu
/// bar in light and dark mode and inverts it while the menu is open. This is a
/// deliberately simplified silhouette of the full-colour app icon: the menu
/// bar wants one flat shape, not the eucalyptus tile and amber accents.
#[cfg(not(target_os = "linux"))]
const TRAY_ICON_PNG: &[u8] = include_bytes!("../icons/tray-icon.png");

/// The same glyph, rendered white instead of black. `icon_as_template` (see
/// [`setup_tray`]) only has an effect on macOS — Linux tray hosts have no
/// equivalent auto-tinting, and most default to a dark panel theme, so the
/// black variant above is invisible there.
#[cfg(target_os = "linux")]
const TRAY_ICON_PNG: &[u8] = include_bytes!("../icons/tray-icon-linux.png");

/// Stable identifiers for every clickable tray menu item. Kept as constants
/// so the menu builder and the event router cannot drift apart.
pub const MENU_ID_DO_DRILL_NOW: &str = "tray_do_drill_now";
pub const MENU_ID_PAUSE_30_MIN: &str = "tray_pause_30min";
pub const MENU_ID_PAUSE_1_HOUR: &str = "tray_pause_1hour";
pub const MENU_ID_PAUSE_CUSTOM: &str = "tray_pause_custom";
pub const MENU_ID_RESUME: &str = "tray_resume";
pub const MENU_ID_TODAYS_STATS: &str = "tray_todays_stats";
pub const MENU_ID_SETTINGS: &str = "tray_settings";
pub const MENU_ID_QUIT: &str = "tray_quit";

const SECS_PER_MINUTE: i64 = 60;

/// On-demand window sizes (logical pixels), each roomy enough for its screen's
/// content without being oversized. Framed and resizable, unlike the toast.
const CUSTOM_PAUSE_SIZE: (f64, f64) = (360.0, 280.0);
const SETTINGS_SIZE: (f64, f64) = (480.0, 560.0);
const STATS_SIZE: (f64, f64) = (560.0, 680.0);

/// What a clicked tray menu item means, decoupled from the raw string id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Surface a nudge immediately (design spec §3.3.1).
    DoDrillNow,
    /// Pause nudges for a fixed number of seconds (the 30 min / 1 hour
    /// presets, design spec §3.3.2).
    PauseFor { secs: i64 },
    /// Open the custom-pause dialog (design spec §3.4).
    OpenCustomPause,
    /// End any active pause immediately (design spec §3.5).
    Resume,
    /// Show today's stats.
    ShowStats,
    /// Open the Settings window (design spec §3.6).
    OpenSettings,
    /// Quit the whole app.
    Quit,
}

impl TrayAction {
    /// The pure core: map a menu item's id to the action it triggers.
    /// Unknown ids yield `None` rather than a default, so a stray event is
    /// ignored loudly-by-omission rather than mis-fired.
    pub fn from_menu_id(id: &str) -> Option<Self> {
        match id {
            MENU_ID_DO_DRILL_NOW => Some(Self::DoDrillNow),
            MENU_ID_PAUSE_30_MIN => Some(Self::PauseFor {
                secs: 30 * SECS_PER_MINUTE,
            }),
            MENU_ID_PAUSE_1_HOUR => Some(Self::PauseFor {
                secs: 60 * SECS_PER_MINUTE,
            }),
            MENU_ID_PAUSE_CUSTOM => Some(Self::OpenCustomPause),
            MENU_ID_RESUME => Some(Self::Resume),
            MENU_ID_TODAYS_STATS => Some(Self::ShowStats),
            MENU_ID_SETTINGS => Some(Self::OpenSettings),
            MENU_ID_QUIT => Some(Self::Quit),
            _ => None,
        }
    }
}

/// Builds the tray menu exactly in the design-spec order.
fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let do_drill_now = MenuItem::with_id(
        app,
        MENU_ID_DO_DRILL_NOW,
        "Do a drill now",
        true,
        None::<&str>,
    )?;

    let pause_30 = MenuItem::with_id(app, MENU_ID_PAUSE_30_MIN, "30 min", true, None::<&str>)?;
    let pause_60 = MenuItem::with_id(app, MENU_ID_PAUSE_1_HOUR, "1 hour", true, None::<&str>)?;
    let pause_custom = MenuItem::with_id(app, MENU_ID_PAUSE_CUSTOM, "Custom…", true, None::<&str>)?;
    let pause_menu = Submenu::with_items(
        app,
        "Pause nudges",
        true,
        &[&pause_30, &pause_60, &pause_custom],
    )?;

    let resume = MenuItem::with_id(app, MENU_ID_RESUME, "Resume nudges", true, None::<&str>)?;
    let todays_stats = MenuItem::with_id(
        app,
        MENU_ID_TODAYS_STATS,
        "Today's stats",
        true,
        None::<&str>,
    )?;
    let settings = MenuItem::with_id(app, MENU_ID_SETTINGS, "Settings…", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, MENU_ID_QUIT, "Quit", true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &do_drill_now,
            &pause_menu,
            &resume,
            &todays_stats,
            &settings,
            &separator,
            &quit,
        ],
    )
}

/// Creates the tray icon and wires its menu events. Called once from the
/// app's `setup` hook.
pub fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    let tray_icon = Image::from_bytes(TRAY_ICON_PNG)
        .expect("the bundled menu-bar glyph decodes as a valid PNG");
    TrayIconBuilder::with_id("budgebell-tray")
        .icon(tray_icon)
        .icon_as_template(true)
        .tooltip("Budgebell")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| handle_menu_event(app, event.id.as_ref()))
        .build(app)?;
    Ok(())
}

/// Routes a menu event to its effect. Kept separate from the closure above so
/// the string-id decoding stays a one-liner over the pure mapping.
fn handle_menu_event(app: &AppHandle, id: &str) {
    let Some(action) = TrayAction::from_menu_id(id) else {
        return;
    };
    match action {
        // Surface a drill immediately in the one shared toast window (design
        // spec §3.3.1) — never a bespoke window — reusing the runtime's
        // present-toast path. Logged, never fatal: a failed on-demand drill
        // must not take the app down.
        TrayAction::DoDrillNow => {
            if let Err(error) = crate::runtime::drill_now(app) {
                eprintln!("the do-a-drill-now request failed: {error}");
            }
        }
        TrayAction::PauseFor { secs } => set_pause_for(app, secs),
        TrayAction::Resume => clear_pause(app),
        TrayAction::OpenCustomPause => open_or_focus(
            app,
            "custom-pause",
            "index.html?view=custom-pause",
            "Pause nudges",
            CUSTOM_PAUSE_SIZE,
        ),
        TrayAction::ShowStats => open_or_focus(
            app,
            "stats",
            "index.html?view=stats",
            "Today's stats",
            STATS_SIZE,
        ),
        TrayAction::OpenSettings => open_or_focus(
            app,
            "settings",
            "index.html?view=settings",
            "Settings",
            SETTINGS_SIZE,
        ),
        TrayAction::Quit => app.exit(0),
    }
}

/// Pauses nudges for `secs` seconds from now by writing the managed pause
/// instant (design spec §3.3.2/§3.7). Mirrors the `pause` command's effect so
/// the tray and the in-app off-switches stay consistent.
fn set_pause_for(app: &AppHandle, secs: i64) {
    let state = app.state::<AppState>();
    let mut guard = state.lock().expect("the app state lock is not poisoned");
    guard.paused_until = Some(Local::now().naive_local() + Duration::seconds(secs));
}

/// Ends any active pause immediately by clearing the managed pause instant
/// (design spec §3.5). Mirrors the `resume` command's effect so a pause started
/// anywhere — the toast's off-switch or a tray preset — can be resumed here.
fn clear_pause(app: &AppHandle) {
    let state = app.state::<AppState>();
    let mut guard = state.lock().expect("the app state lock is not poisoned");
    guard.paused_until = None;
}

/// Shows an existing labelled window (bringing it to the front) or builds it
/// at the given size if it does not exist yet. These are ordinary framed,
/// resizable app windows — the transparent, frameless toast is built by the
/// runtime, not here.
fn open_or_focus(app: &AppHandle, label: &str, url: &str, title: &str, size: (f64, f64)) {
    if let Some(window) = app.get_webview_window(label) {
        window.show().expect("the window shows");
        window.set_focus().expect("the window takes focus");
        return;
    }
    let (width, height) = size;
    WebviewWindowBuilder::new(app, label, WebviewUrl::App(url.into()))
        .title(title)
        .inner_size(width, height)
        .resizable(true)
        .build()
        .expect("the window builds");
}

#[cfg(test)]
mod tests {
    use super::*;

    // Given each known tray menu id, when decoded, then it maps to the
    // matching action in the design-spec order.
    #[test]
    fn maps_do_drill_now() {
        assert_eq!(
            TrayAction::from_menu_id(MENU_ID_DO_DRILL_NOW),
            Some(TrayAction::DoDrillNow)
        );
    }

    // Given the 30-minute preset, when decoded, then it pauses for 1800s.
    #[test]
    fn maps_pause_30_min_to_1800_seconds() {
        assert_eq!(
            TrayAction::from_menu_id(MENU_ID_PAUSE_30_MIN),
            Some(TrayAction::PauseFor { secs: 1800 })
        );
    }

    // Given the 1-hour preset, when decoded, then it pauses for 3600s.
    #[test]
    fn maps_pause_1_hour_to_3600_seconds() {
        assert_eq!(
            TrayAction::from_menu_id(MENU_ID_PAUSE_1_HOUR),
            Some(TrayAction::PauseFor { secs: 3600 })
        );
    }

    // Given the custom-pause item, when decoded, then it opens the dialog.
    #[test]
    fn maps_custom_pause() {
        assert_eq!(
            TrayAction::from_menu_id(MENU_ID_PAUSE_CUSTOM),
            Some(TrayAction::OpenCustomPause)
        );
    }

    // Given the resume item, when decoded, then it resumes nudges.
    #[test]
    fn maps_resume() {
        assert_eq!(
            TrayAction::from_menu_id(MENU_ID_RESUME),
            Some(TrayAction::Resume)
        );
    }

    // Given the stats item, when decoded, then it shows stats.
    #[test]
    fn maps_todays_stats() {
        assert_eq!(
            TrayAction::from_menu_id(MENU_ID_TODAYS_STATS),
            Some(TrayAction::ShowStats)
        );
    }

    // Given the settings item, when decoded, then it opens Settings.
    #[test]
    fn maps_settings() {
        assert_eq!(
            TrayAction::from_menu_id(MENU_ID_SETTINGS),
            Some(TrayAction::OpenSettings)
        );
    }

    // Given the quit item, when decoded, then it quits.
    #[test]
    fn maps_quit() {
        assert_eq!(
            TrayAction::from_menu_id(MENU_ID_QUIT),
            Some(TrayAction::Quit)
        );
    }

    // Given a menu id that belongs to no tray item, when decoded, then
    // nothing fires (loud-by-omission).
    #[test]
    fn unknown_id_maps_to_none() {
        assert_eq!(TrayAction::from_menu_id("not_a_tray_item"), None);
        assert_eq!(TrayAction::from_menu_id(""), None);
    }
}
