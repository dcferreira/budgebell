// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

mod commands;

// Large swathes of the store's and domain model's CRUD/construction surface
// (inserting/updating habits and rotations, listing events, building
// triggers from scratch) aren't reachable from the commands wired up so
// far — they're consumed by the `seed-wire` and `mcp` tasks still to come.
#[allow(dead_code, unused_imports)]
mod domain;
// The in-process MCP server (design spec §6). Its stdio entry point is wired
// up conditionally in `run()`; the tool handlers and DTO surface beyond what
// that path touches are exercised by the module's own tests.
#[allow(dead_code, unused_imports)]
mod mcp;
// Pure resolution of a habit's relative `media_path` onto an absolute path
// under the media directory (design spec §4.1), used at the `present_toast`
// choke point in `runtime.rs`.
mod media;
// One-off move of the pre-rebrand (`habits`) app data onto Budgebell's paths.
mod legacy_data;
// The idle/DND/EventKit probes and their pure parsing helpers are exercised
// by the `list_due` command and the module's own unit tests; the broader
// helper surface is consumed as the app grows.
#[allow(dead_code, unused_imports)]
mod quiet_os;
mod runtime;
mod scheduler;
// The stats data path (design spec §6.1): pure day-summary/longest-gap
// aggregation plus the shared date-ranged query, reused by the `day_log`
// Tauri command and the MCP `day_log` tool.
mod stats;
// First-run seeding (design spec §7); called once from `run()`, so its
// internals aren't otherwise reachable.
#[allow(dead_code, unused_imports)]
mod seed;
#[allow(dead_code, unused_imports)]
mod store;
mod tray;

use tauri::Manager;

use commands::{
    complete_habit, current_due, day_log, get_config, list_due, list_habits, pause, resume,
    set_config, skip_habit, snooze_habit, AppState,
};
use store::Store;

/// Smoke-test command wired through the IPC bridge to prove the Rust <-> UI
/// round trip works, kept alongside the real habit commands below.
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! You've been greeted from Rust!")
}

/// The bundle identifier (mirrors `tauri.conf.json`'s `identifier`). The
/// headless MCP server resolves its DB path from it so it lands on the very
/// same SQLite file Tauri's `app_data_dir()` gives the GUI.
const APP_IDENTIFIER: &str = "com.dcferreira.budgebell";

/// The on-device SQLite file inside the app-data directory.
const DB_FILE: &str = "budgebell.sqlite";

/// The on-device SQLite path for the headless MCP server. Honours a
/// `BUDGEBELL_DB_PATH` override — a test seam letting an end-to-end test point at
/// a throwaway database — and otherwise uses the platform data directory joined
/// with the bundle identifier, matching Tauri's `app_data_dir()` on macOS.
fn mcp_db_path() -> std::path::PathBuf {
    if let Some(path) = std::env::var_os("BUDGEBELL_DB_PATH") {
        return std::path::PathBuf::from(path);
    }
    let data_root = dirs::data_dir().expect("a platform data directory");
    legacy_data::migrate(&data_root, APP_IDENTIFIER, DB_FILE)
        .expect("the pre-rebrand app data migrates");
    data_root.join(APP_IDENTIFIER).join(DB_FILE)
}

/// Runs the local MCP server over stdio to completion (design spec §6). It
/// opens its own connection to the shared on-device store and serves until the
/// client disconnects, at which point this returns and the process exits — no
/// GUI, tray, or scheduler is ever started in this mode.
fn run_mcp_stdio() {
    let db_path = mcp_db_path();
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).expect("the MCP database directory is creatable");
    }
    let store = Store::open(&db_path).expect("the MCP store opens");
    let shared = std::sync::Arc::new(std::sync::Mutex::new(store));
    tauri::async_runtime::block_on(async move {
        if let Err(error) = mcp::serve_stdio(shared).await {
            eprintln!("the MCP stdio server exited with an error: {error}");
        }
    });
}

/// WebKitGTK's DMA-BUF/GBM compositing path crashes with a Wayland protocol
/// error ("Error 71") on NVIDIA's proprietary driver, triggered the first
/// time a transparent, hardware-composited window (the toast) is shown.
/// Disabling the DMA-BUF renderer avoids it; it must be set before WebKitGTK
/// initializes, which happens as a side effect of the first `tauri::Builder`
/// call below. Respects an existing value so a user/packager can override it.
#[cfg(target_os = "linux")]
fn apply_wayland_dmabuf_workaround() {
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        // SAFETY: called once, synchronously, at startup before any other
        // thread exists.
        unsafe {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
    }
}

/// Forces GDK to run under XWayland instead of native Wayland (see
/// `runtime::position_top_right`'s doc comment for why). Wayland's core
/// protocol does not let a regular client window set its own absolute screen
/// position at all — by design, not an oversight — so the toast can never be
/// pinned to a screen corner there; `window.primary_monitor()` returns `None`
/// on native Wayland for the same reason. X11 (via XWayland) does support
/// this. A real native-Wayland fix (the wlr layer-shell protocol) was
/// investigated and ruled out: it explicitly does not work under GNOME/Mutter
/// at all, which is what most Linux desktop users run, so it wouldn't fix the
/// common case regardless of the implementation effort. Must be set before
/// GDK initializes, i.e. before the first `tauri::Builder` call below.
/// Respects an existing value so a user/packager who wants native Wayland
/// (accepting the centered toast) can still force it back with
/// `GDK_BACKEND=wayland`.
#[cfg(target_os = "linux")]
fn apply_xwayland_workaround() {
    if std::env::var_os("GDK_BACKEND").is_none() {
        // SAFETY: called once, synchronously, at startup before any other
        // thread exists.
        unsafe {
            std::env::set_var("GDK_BACKEND", "x11");
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Headless MCP mode (design spec §6): with BUDGEBELL_MCP_STDIO set, act as a
    // pure local stdio MCP server — no GUI, no tray, no scheduler — reading and
    // writing the same on-device SQLite the GUI uses, and exiting cleanly when
    // the client disconnects. This is how a locally-running LLM (or an MCP
    // client such as Claude Code) launches the app to manage habits. Branching
    // here, before the Tauri builder, keeps the GUI out of the stdio stream and
    // lets the process terminate on disconnect.
    if std::env::var_os("BUDGEBELL_MCP_STDIO").is_some() {
        run_mcp_stdio();
        return;
    }

    #[cfg(target_os = "linux")]
    apply_wayland_dmabuf_workaround();
    #[cfg(target_os = "linux")]
    apply_xwayland_workaround();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // A menu-bar agent (design spec §3.3): no Dock icon, no app menu —
            // the app lives entirely in the tray and only ever shows windows on
            // demand. `Accessory` is the macOS activation policy for exactly
            // that kind of background/agent app.
            #[cfg(target_os = "macos")]
            let _ = app
                .handle()
                .set_activation_policy(tauri::ActivationPolicy::Accessory);

            // The app-data directory and the SQLite file within it are the
            // only local state this app has — resolving/creating them is
            // this app's foundation, so a failure here is unrecoverable and
            // should crash loudly rather than silently limp along.
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("the app data directory resolves");
            // Carry over data from the pre-rebrand `habits` app before anything
            // creates a fresh (empty) directory in its place.
            if let Some(data_root) = app_data_dir.parent() {
                legacy_data::migrate(data_root, APP_IDENTIFIER, DB_FILE)
                    .expect("the pre-rebrand app data migrates");
            }
            std::fs::create_dir_all(&app_data_dir).expect("the app data directory is creatable");
            let db_path = app_data_dir.join(DB_FILE);
            let store = Store::open(&db_path).expect("the store opens");

            // The scoped media folder (design spec §3) that habit images/videos
            // are read from — created up front so it exists before any due
            // habit's media is resolved. A failure here is as unrecoverable as
            // the app-data directory itself, so it fails loudly too.
            let media_dir = app_data_dir.join("media");
            std::fs::create_dir_all(&media_dir).expect("the media directory is creatable");

            // Seed the default content (rotation, drills, strength session,
            // config) on first run (design spec §7). A no-op once seeded, so
            // re-launches never duplicate content.
            let created_at = chrono::Local::now().timestamp();
            seed::seed_if_empty(&store, created_at).expect("seeding the default content succeeds");

            app.manage(AppState::new(store));

            // The MCP server (design spec §6) is not started here: with
            // BUDGEBELL_MCP_STDIO set the process never reaches the GUI builder
            // (see `run`), running headless instead. A normal GUI launch has no
            // MCP server and never touches stdin/stdout.

            // The macOS menu-bar tray (design spec §3.3) — its menu events
            // drive the pause off-switch and open the app's windows.
            tray::setup_tray(app.handle()).expect("the tray icon is created");

            // The runtime bridge (design spec §10 "seed-wire"): a background
            // scheduler tick that surfaces the corner toast when a habit is
            // due.
            runtime::spawn_scheduler_tick(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            list_due,
            complete_habit,
            skip_habit,
            snooze_habit,
            pause,
            resume,
            list_habits,
            current_due,
            get_config,
            set_config,
            day_log,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            // A tray-only menu-bar app must outlive its windows: closing the
            // last window (Settings, Stats, the toast, …) must not quit the
            // process. Only the tray's Quit item — which calls `app.exit(0)` —
            // ends it. So we veto only the "last window closed" exit request
            // here, letting a programmatic `app.exit`/`app.restart` through.
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if should_veto_exit(code) {
                    api.prevent_exit();
                }
            }
        });
}

/// Whether a run-loop exit request should be vetoed. `code` is `None` when
/// the request comes from user interaction (e.g. closing the last window) —
/// exactly the case a tray-only app must survive — and `Some` when it was
/// requested programmatically via `app.exit`/`app.restart` (the tray's Quit
/// item), which must always be allowed to actually end the process.
fn should_veto_exit(code: Option<i32>) -> bool {
    code.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greet_includes_the_name() {
        assert_eq!(greet("Ada"), "Hello, Ada! You've been greeted from Rust!");
    }

    #[test]
    fn a_window_close_exit_request_is_vetoed() {
        // Given an exit request with no code (a window closed by the user)
        // Then it is vetoed, so the tray-only app survives
        assert!(should_veto_exit(None));
    }

    #[test]
    fn a_programmatic_exit_request_is_allowed_through() {
        // Given an exit request with a code (the tray's Quit item calling
        // `app.exit(0)`)
        // Then it is allowed through, so Quit actually quits
        assert!(!should_veto_exit(Some(0)));
    }
}
