//! Full-screen-foreground probe. "Don't nudge over a game, a film or a
//! slide deck": while the focused window fills its screen, a toast would land
//! on top of whatever the user is absorbed in, so the trigger is held like
//! every other quiet source. Fully local — no network I/O. The classifiers
//! here are pure and unit-tested; only `probe_fullscreen` touches the OS.
//!
//! **macOS.** A tiny ad-hoc-signed Swift helper (`helpers/fullscreen_probe.swift`,
//! compiled by `build.rs` and bundled as a sidecar) prints, as JSON, the
//! frontmost application's pid, every active display's bounds (with the
//! camera-housing inset of notched displays) and the on-screen windows'
//! owner pid, layer and bounds. Window bounds and owners need no Screen
//! Recording permission (only window *titles* do), so no TCC prompt.
//! [`parse_macos_fullscreen`] then decides: the frontmost app owns a normal
//! (layer 0) or raised (exclusive-mode game) window that covers a whole display — or a whole display below
//! the notch, where macOS places native full-screen windows by default.
//!
//! **Linux.** The EWMH hints are read with `xprop`: the root window's
//! `_NET_ACTIVE_WINDOW`, then that window's `_NET_WM_STATE`, which carries
//! `_NET_WM_STATE_FULLSCREEN`. When the window manager advertises
//! `_NET_WM_STATE_FOCUSED`, the window must also carry it: under GNOME
//! Wayland the X11 active-window hint can keep naming an XWayland window
//! after focus has moved to a native Wayland one. This covers X11 sessions
//! and every XWayland window on Wayland — which includes Steam/Proton
//! games. Native Wayland windows are invisible to it: GNOME Shell denies
//! window introspection (`org.gnome.Shell.Introspect.GetWindows`) to
//! ordinary clients, so a full-screen native-Wayland video player is not
//! detected. A missing `xprop` or no X display degrades to "not full
//! screen", like the other Linux probes.

#[cfg(target_os = "macos")]
use std::process::Command;

use serde_json::Value;

use super::error::QuietOsError;
#[cfg(target_os = "macos")]
use super::probe_path;
#[cfg(target_os = "linux")]
use super::tool_output::run_stdout_if_available;

/// Slack, in points, when comparing a window's bounds with a display's: the
/// window server reports fractional bounds on scaled displays.
const BOUNDS_TOLERANCE: f64 = 1.0;

/// What `xprop -root _NET_ACTIVE_WINDOW _NET_SUPPORTED` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XRootHints {
    /// The focused client window's id (e.g. `0x5a00003`), or `None` when no
    /// window is active (`0x0`) or the window manager does not set the hint.
    pub active_window: Option<String>,
    /// Whether the window manager maintains `_NET_WM_STATE_FOCUSED`.
    pub wm_reports_focus: bool,
}

/// What `xprop -id <window> _NET_WM_STATE` says about one window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct XWindowState {
    pub fullscreen: bool,
    pub focused: bool,
}

/// Parses `xprop -root _NET_ACTIVE_WINDOW _NET_SUPPORTED` output. A missing
/// property (`_NET_ACTIVE_WINDOW:  not found.`) or the `0x0` id means no
/// active window. Output with no `_NET_ACTIVE_WINDOW` line at all fails
/// loudly.
pub fn parse_xprop_root(output: &str) -> Result<XRootHints, QuietOsError> {
    let active_line = output
        .lines()
        .find(|line| line.starts_with("_NET_ACTIVE_WINDOW"))
        .ok_or_else(|| QuietOsError::UnparsableFullscreen(output.to_string()))?;
    let active_window = match active_line.split_once("window id # ") {
        Some((_, id)) => {
            let id = id.trim();
            if !is_window_id(id) {
                return Err(QuietOsError::UnparsableFullscreen(output.to_string()));
            }
            (u64::from_str_radix(&id[2..], 16).ok() != Some(0)).then(|| id.to_string())
        }
        None if active_line.ends_with("not found.") => None,
        None => return Err(QuietOsError::UnparsableFullscreen(output.to_string())),
    };
    let wm_reports_focus = output
        .lines()
        .filter(|line| line.starts_with("_NET_SUPPORTED"))
        .any(|line| atoms(line).any(|atom| atom == "_NET_WM_STATE_FOCUSED"));
    Ok(XRootHints {
        active_window,
        wm_reports_focus,
    })
}

/// Parses `xprop -id <window> _NET_WM_STATE` output. An empty or missing
/// state means neither flag; output with no `_NET_WM_STATE` line fails
/// loudly.
pub fn parse_xprop_window_state(output: &str) -> Result<XWindowState, QuietOsError> {
    let line = output
        .lines()
        .find(|line| line.starts_with("_NET_WM_STATE"))
        .ok_or_else(|| QuietOsError::UnparsableFullscreen(output.to_string()))?;
    let mut state = XWindowState::default();
    for atom in atoms(line) {
        match atom {
            "_NET_WM_STATE_FULLSCREEN" => state.fullscreen = true,
            "_NET_WM_STATE_FOCUSED" => state.focused = true,
            _ => {}
        }
    }
    Ok(state)
}

/// Whether the active X window is full screen and really holds the focus.
pub fn is_x11_fullscreen_foreground(root: &XRootHints, window: XWindowState) -> bool {
    root.active_window.is_some() && window.fullscreen && (window.focused || !root.wm_reports_focus)
}

/// The comma-separated atoms after `=` on an xprop `(ATOM) = …` line.
fn atoms(line: &str) -> impl Iterator<Item = &str> {
    line.split_once('=')
        .map_or("", |(_, rest)| rest)
        .split(',')
        .map(str::trim)
        .filter(|atom| !atom.is_empty())
}

fn is_window_id(id: &str) -> bool {
    id.len() > 2 && id.starts_with("0x") && id[2..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Whether the macOS helper's JSON describes a full-screen frontmost app:
/// the frontmost pid owns a window at the normal layer or above (exclusive-mode
/// games sit at the shielding level) covering a whole display, or a
/// whole display below its top (notch) inset. A `null` frontmost pid (no
/// frontmost app) is not full screen. JSON of the wrong shape fails loudly.
pub fn parse_macos_fullscreen(json: &str) -> Result<bool, QuietOsError> {
    let parsed: Value = serde_json::from_str(json).map_err(|source| QuietOsError::Json {
        probe: "fullscreen",
        source,
    })?;
    let unparsable = || QuietOsError::UnparsableFullscreen(json.to_string());

    let frontmost = parsed.get("frontmost_pid").ok_or_else(unparsable)?;
    if frontmost.is_null() {
        return Ok(false);
    }
    let frontmost = frontmost.as_i64().ok_or_else(unparsable)?;
    let displays = parsed
        .get("displays")
        .and_then(Value::as_array)
        .ok_or_else(unparsable)?
        .iter()
        .map(|display| {
            Some(Display {
                bounds: Rect::from_json(display)?,
                top_inset: display.get("top_inset")?.as_f64()?,
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(unparsable)?;
    let windows = parsed
        .get("windows")
        .and_then(Value::as_array)
        .ok_or_else(unparsable)?;

    for window in windows {
        let (Some(pid), Some(layer), Some(bounds)) = (
            window.get("pid").and_then(Value::as_i64),
            window.get("layer").and_then(Value::as_i64),
            Rect::from_json(window),
        ) else {
            return Err(unparsable());
        };
        if pid == frontmost && layer >= 0 && displays.iter().any(|d| d.is_covered_by(&bounds)) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

impl Rect {
    fn from_json(value: &Value) -> Option<Self> {
        Some(Self {
            x: value.get("x")?.as_f64()?,
            y: value.get("y")?.as_f64()?,
            width: value.get("width")?.as_f64()?,
            height: value.get("height")?.as_f64()?,
        })
    }

    fn matches(&self, other: &Rect) -> bool {
        let close = |a: f64, b: f64| (a - b).abs() <= BOUNDS_TOLERANCE;
        close(self.x, other.x)
            && close(self.y, other.y)
            && close(self.width, other.width)
            && close(self.height, other.height)
    }
}

/// A display in the window server's top-left-origin coordinates.
#[derive(Debug, Clone, Copy)]
struct Display {
    bounds: Rect,
    /// The camera-housing (notch) inset; 0 on displays without one.
    top_inset: f64,
}

impl Display {
    fn is_covered_by(&self, window: &Rect) -> bool {
        let below_notch = Rect {
            y: self.bounds.y + self.top_inset,
            height: self.bounds.height - self.top_inset,
            ..self.bounds
        };
        window.matches(&self.bounds) || (self.top_inset > 0.0 && window.matches(&below_notch))
    }
}

#[cfg(target_os = "macos")]
pub fn probe_fullscreen() -> Result<bool, QuietOsError> {
    let probe = probe_path::resolve("fullscreen_probe").map_err(|source| QuietOsError::Spawn {
        probe: "fullscreen",
        source,
    })?;
    let output = Command::new(probe)
        .output()
        .map_err(|source| QuietOsError::Spawn {
            probe: "fullscreen",
            source,
        })?;
    if !output.status.success() {
        return Err(QuietOsError::ProbeFailed {
            probe: "fullscreen",
            status: output.status.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    parse_macos_fullscreen(&String::from_utf8_lossy(&output.stdout))
}

#[cfg(target_os = "linux")]
pub fn probe_fullscreen() -> Result<bool, QuietOsError> {
    probe_fullscreen_with(run_stdout_if_available)
}

/// The Linux probe with the tool runner injected, so the degrade and error
/// handling can be tested without an X server.
#[cfg(target_os = "linux")]
fn probe_fullscreen_with(
    run: impl Fn(&str, &[&str]) -> Option<String>,
) -> Result<bool, QuietOsError> {
    // No `xprop`, or no X display to talk to (a pure Wayland session without
    // XWayland), exits nonzero or fails to spawn: degrade to "not full
    // screen" rather than failing every scheduler tick.
    let Some(root) = run("xprop", &["-root", "_NET_ACTIVE_WINDOW", "_NET_SUPPORTED"]) else {
        return Ok(false);
    };
    let root = parse_xprop_root(&root)?;
    let Some(window) = root.active_window.as_deref() else {
        return Ok(false);
    };
    // The window can close between the two calls; xprop then exits nonzero.
    let Some(state) = run("xprop", &["-id", window, "_NET_WM_STATE"]) else {
        return Ok(false);
    };
    Ok(is_x11_fullscreen_foreground(
        &root,
        parse_xprop_window_state(&state)?,
    ))
}

// Not implemented on this platform: degrades to "not full screen", like the
// other probes, so reminders keep working.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn probe_fullscreen() -> Result<bool, QuietOsError> {
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real `LC_ALL=C xprop` output captured on GNOME 51 (Wayland) with a
    // Proton game running full screen under XWayland.
    const ROOT_GAME_ACTIVE: &str = "_NET_ACTIVE_WINDOW(WINDOW): window id # 0x5a00003\n\
        _NET_SUPPORTED(ATOM) = _NET_WM_NAME, _NET_CLOSE_WINDOW, _NET_WM_STATE, \
        _NET_WM_STATE_MAXIMIZED_HORZ, _NET_WM_STATE_FULLSCREEN, _NET_WM_STATE_FOCUSED, \
        _NET_ACTIVE_WINDOW\n";
    const STATE_FULLSCREEN_FOCUSED: &str =
        "_NET_WM_STATE(ATOM) = _NET_WM_STATE_FULLSCREEN, _NET_WM_STATE_FOCUSED\n";
    const STATE_EMPTY: &str = "_NET_WM_STATE(ATOM) = \n";

    fn root(active: Option<&str>, wm_reports_focus: bool) -> XRootHints {
        XRootHints {
            active_window: active.map(str::to_string),
            wm_reports_focus,
        }
    }

    #[test]
    fn the_root_hints_name_the_active_window_and_the_focus_support() {
        // Given GNOME's root-window hints with a game active
        // When parsed
        let hints = parse_xprop_root(ROOT_GAME_ACTIVE).expect("parses");

        // Then the active window and the focused-state support are read
        assert_eq!(hints, root(Some("0x5a00003"), true));
    }

    #[test]
    fn a_zero_or_missing_active_window_means_none_is_active() {
        // Given a WM reporting no active window, and one not setting the hint
        let zero =
            "_NET_ACTIVE_WINDOW(WINDOW): window id # 0x0\n_NET_SUPPORTED(ATOM) = _NET_WM_STATE\n";
        let missing = "_NET_ACTIVE_WINDOW:  not found.\n_NET_SUPPORTED:  not found.\n";

        // When parsed
        // Then no window is active and focus is not reported
        assert_eq!(parse_xprop_root(zero).expect("parses"), root(None, false));
        assert_eq!(
            parse_xprop_root(missing).expect("parses"),
            root(None, false)
        );
    }

    #[test]
    fn root_output_without_the_active_window_line_fails_loudly() {
        // Given output that is not what xprop prints for the root hints
        for garbage in [
            "",
            "something else\n",
            "_NET_ACTIVE_WINDOW(WINDOW): window id # zz\n",
        ] {
            // When parsed
            // Then it is a loud error rather than a guess
            assert!(
                matches!(
                    parse_xprop_root(garbage),
                    Err(QuietOsError::UnparsableFullscreen(_))
                ),
                "{garbage:?}"
            );
        }
    }

    #[test]
    fn the_window_state_reads_the_fullscreen_and_focused_flags() {
        // Given a full-screen focused window, a plain window and a stateless one
        // When parsed
        // Then the flags are read
        assert_eq!(
            parse_xprop_window_state(STATE_FULLSCREEN_FOCUSED).expect("parses"),
            XWindowState {
                fullscreen: true,
                focused: true
            }
        );
        assert_eq!(
            parse_xprop_window_state(STATE_EMPTY).expect("parses"),
            XWindowState::default()
        );
        assert_eq!(
            parse_xprop_window_state("_NET_WM_STATE:  not found.\n").expect("parses"),
            XWindowState::default()
        );
        assert_eq!(
            parse_xprop_window_state(
                "_NET_WM_STATE(ATOM) = _NET_WM_STATE_MAXIMIZED_VERT, _NET_WM_STATE_MAXIMIZED_HORZ, _NET_WM_STATE_FOCUSED\n"
            )
            .expect("parses"),
            XWindowState {
                fullscreen: false,
                focused: true
            }
        );
    }

    #[test]
    fn window_state_output_without_the_state_line_fails_loudly() {
        assert!(matches!(
            parse_xprop_window_state("WM_CLASS(STRING) = \"x\"\n"),
            Err(QuietOsError::UnparsableFullscreen(_))
        ));
    }

    #[test]
    fn a_focused_fullscreen_window_is_a_fullscreen_foreground() {
        let window = XWindowState {
            fullscreen: true,
            focused: true,
        };
        assert!(is_x11_fullscreen_foreground(
            &root(Some("0x1"), true),
            window
        ));
    }

    #[test]
    fn a_fullscreen_window_that_lost_focus_to_a_wayland_window_is_not() {
        // Given GNOME still naming a full-screen XWayland game as active, but
        // the game no longer carrying _NET_WM_STATE_FOCUSED
        let window = XWindowState {
            fullscreen: true,
            focused: false,
        };

        // Then the user is not in the game
        assert!(!is_x11_fullscreen_foreground(
            &root(Some("0x1"), true),
            window
        ));
    }

    #[test]
    fn without_focus_reporting_the_fullscreen_flag_alone_decides() {
        // Given a WM that does not maintain _NET_WM_STATE_FOCUSED
        let window = XWindowState {
            fullscreen: true,
            focused: false,
        };

        // Then a full-screen active window counts
        assert!(is_x11_fullscreen_foreground(
            &root(Some("0x1"), false),
            window
        ));
    }

    #[test]
    fn a_focused_windowed_app_is_not_a_fullscreen_foreground() {
        let window = XWindowState {
            fullscreen: false,
            focused: true,
        };
        assert!(!is_x11_fullscreen_foreground(
            &root(Some("0x1"), true),
            window
        ));
    }

    fn macos_json(frontmost: &str, windows: &str) -> String {
        format!(
            r#"{{"frontmost_pid": {frontmost},
                "displays": [
                    {{"x": 0, "y": 0, "width": 1512, "height": 982, "top_inset": 32}},
                    {{"x": 1512, "y": -200, "width": 2560, "height": 1440, "top_inset": 0}}
                ],
                "windows": [{windows}]}}"#
        )
    }

    #[test]
    fn a_frontmost_window_covering_an_external_display_is_full_screen() {
        // Given the frontmost app owns a window exactly covering the external display
        let json = macos_json(
            "42",
            r#"{"pid": 42, "layer": 0, "x": 1512, "y": -200, "width": 2560, "height": 1440}"#,
        );

        // Then it is full screen
        assert!(parse_macos_fullscreen(&json).expect("parses"));
    }

    #[test]
    fn a_frontmost_window_filling_a_notched_display_below_the_notch_is_full_screen() {
        // Given native full screen on a notched MacBook, placed below the camera housing
        let below_notch =
            r#"{"pid": 42, "layer": 0, "x": 0, "y": 32, "width": 1512, "height": 950}"#;
        let whole = r#"{"pid": 42, "layer": 0, "x": 0, "y": 0, "width": 1512, "height": 982}"#;

        // Then both placements count as full screen
        assert!(parse_macos_fullscreen(&macos_json("42", below_notch)).expect("parses"));
        assert!(parse_macos_fullscreen(&macos_json("42", whole)).expect("parses"));
    }

    #[test]
    fn a_zoomed_window_below_the_menu_bar_is_not_full_screen() {
        // Given a maximised (zoomed) window on the external display, which
        // leaves the menu bar visible
        let json = macos_json(
            "42",
            r#"{"pid": 42, "layer": 0, "x": 1512, "y": -175, "width": 2560, "height": 1415}"#,
        );

        // Then it is not full screen
        assert!(!parse_macos_fullscreen(&json).expect("parses"));
    }

    #[test]
    fn a_full_display_window_of_a_background_app_or_below_normal_layer_does_not_count() {
        // Given a display-sized window owned by another app, and a
        // display-sized desktop-level window (negative layer) owned by the
        // frontmost app
        let other_app =
            r#"{"pid": 7, "layer": 0, "x": 1512, "y": -200, "width": 2560, "height": 1440}"#;
        let desktop =
            r#"{"pid": 42, "layer": -2147483623, "x": 0, "y": 0, "width": 1512, "height": 982}"#;

        // Then neither is a full-screen frontmost app
        assert!(!parse_macos_fullscreen(&macos_json("42", other_app)).expect("parses"));
        assert!(!parse_macos_fullscreen(&macos_json("42", desktop)).expect("parses"));
    }

    #[test]
    fn an_exclusive_mode_game_at_the_shielding_level_is_full_screen() {
        // Given a game in exclusive full screen (SDL / CGDisplayCapture),
        // whose window sits at CGShieldingWindowLevel rather than layer 0
        let shield = r#"{"pid": 42, "layer": 2147483630, "x": 1512, "y": -200, "width": 2560, "height": 1440}"#;

        // Then it is full screen
        assert!(parse_macos_fullscreen(&macos_json("42", shield)).expect("parses"));
    }

    #[test]
    fn fractional_bounds_on_scaled_displays_are_tolerated() {
        let json = macos_json(
            "42",
            r#"{"pid": 42, "layer": 0, "x": 1512.0, "y": -199.5, "width": 2560.5, "height": 1440}"#,
        );
        assert!(parse_macos_fullscreen(&json).expect("parses"));
    }

    #[test]
    fn no_frontmost_app_is_not_full_screen() {
        let json = macos_json(
            "null",
            r#"{"pid": 42, "layer": 0, "x": 0, "y": 0, "width": 1512, "height": 982}"#,
        );
        assert!(!parse_macos_fullscreen(&json).expect("parses"));
    }

    #[test]
    fn malformed_helper_json_fails_loudly() {
        // Given output that is not JSON, and JSON of the wrong shape
        assert!(matches!(
            parse_macos_fullscreen("1"),
            Err(QuietOsError::UnparsableFullscreen(_))
        ));
        assert!(matches!(
            parse_macos_fullscreen("not json"),
            Err(QuietOsError::Json { .. })
        ));
        assert!(matches!(
            parse_macos_fullscreen(&macos_json("42", r#"{"pid": 42}"#)),
            Err(QuietOsError::UnparsableFullscreen(_))
        ));
    }

    #[cfg(target_os = "linux")]
    mod linux {
        use super::*;
        use std::cell::RefCell;

        /// A fake tool runner answering the two xprop calls.
        fn xprop_runner<'a>(
            root: Option<&'a str>,
            state: Option<&'a str>,
            calls: &'a RefCell<Vec<Vec<String>>>,
        ) -> impl Fn(&str, &[&str]) -> Option<String> + 'a {
            move |program, args| {
                assert_eq!(program, "xprop");
                calls
                    .borrow_mut()
                    .push(args.iter().map(|a| a.to_string()).collect());
                if args.first() == Some(&"-root") {
                    root.map(str::to_string)
                } else {
                    state.map(str::to_string)
                }
            }
        }

        #[test]
        fn a_focused_fullscreen_game_is_detected_end_to_end() {
            let calls = RefCell::new(Vec::new());
            let run = xprop_runner(
                Some(ROOT_GAME_ACTIVE),
                Some(STATE_FULLSCREEN_FOCUSED),
                &calls,
            );

            assert!(probe_fullscreen_with(run).expect("probes"));
            assert_eq!(calls.borrow()[1], vec!["-id", "0x5a00003", "_NET_WM_STATE"]);
        }

        #[test]
        fn no_x_display_or_xprop_degrades_to_not_fullscreen() {
            let calls = RefCell::new(Vec::new());
            let run = xprop_runner(None, None, &calls);

            assert!(!probe_fullscreen_with(run).expect("degrades"));
        }

        #[test]
        fn a_window_that_closed_between_the_calls_degrades_to_not_fullscreen() {
            let calls = RefCell::new(Vec::new());
            let run = xprop_runner(Some(ROOT_GAME_ACTIVE), None, &calls);

            assert!(!probe_fullscreen_with(run).expect("degrades"));
        }

        #[test]
        fn no_active_window_skips_the_window_query() {
            let calls = RefCell::new(Vec::new());
            let run = xprop_runner(
                Some("_NET_ACTIVE_WINDOW(WINDOW): window id # 0x0\n"),
                Some(STATE_FULLSCREEN_FOCUSED),
                &calls,
            );

            assert!(!probe_fullscreen_with(run).expect("probes"));
            assert_eq!(calls.borrow().len(), 1);
        }

        #[test]
        fn focus_on_a_native_wayland_window_is_not_fullscreen() {
            // Given GNOME after focus moved from an XWayland game to a native
            // Wayland window: the active hint names a stateless internal
            // window (observed live on GNOME 51)
            let calls = RefCell::new(Vec::new());
            let run = xprop_runner(
                Some(
                    "_NET_ACTIVE_WINDOW(WINDOW): window id # 0x200003\n\
                     _NET_SUPPORTED(ATOM) = _NET_WM_STATE, _NET_WM_STATE_FOCUSED\n",
                ),
                Some("_NET_WM_STATE:  not found.\n"),
                &calls,
            );

            // Then nothing full screen holds the foreground
            assert!(!probe_fullscreen_with(run).expect("probes"));
        }

        #[test]
        fn garbage_from_an_xprop_that_answered_fails_loudly() {
            let calls = RefCell::new(Vec::new());
            let run = xprop_runner(Some("garbage\n"), None, &calls);

            assert!(matches!(
                probe_fullscreen_with(run),
                Err(QuietOsError::UnparsableFullscreen(_))
            ));
        }
    }
}
