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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // The app-data directory and the SQLite file within it are the
            // only local state this app has — resolving/creating them is
            // this app's foundation, so a failure here is unrecoverable and
            // should crash loudly rather than silently limp along.
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("the app data directory resolves");
            std::fs::create_dir_all(&app_data_dir).expect("the app data directory is creatable");
            let db_path = app_data_dir.join("habits.sqlite");
            let store = Store::open(&db_path).expect("the store opens");

            // Seed the default content (rotation, drills, strength session,
            // config) on first run (design spec §7). A no-op once seeded, so
            // re-launches never duplicate content.
            let created_at = chrono::Local::now().timestamp();
            seed::seed_if_empty(&store, created_at)
                .expect("seeding the default content succeeds");

            app.manage(AppState::new(store));

            // The in-process MCP server (design spec §6), local transport
            // only. It is off by default so a normal GUI launch never touches
            // stdin/stdout; a locally-running LLM launches the app with
            // HABITS_MCP_STDIO set to speak MCP over stdio. It opens its own
            // connection to the same on-device SQLite file — nothing leaves
            // the machine.
            if std::env::var_os("HABITS_MCP_STDIO").is_some() {
                let mcp_store = Store::open(&db_path).expect("the MCP store opens");
                let shared = std::sync::Arc::new(std::sync::Mutex::new(mcp_store));
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = mcp::serve_stdio(shared).await {
                        eprintln!("the MCP stdio server exited with an error: {error}");
                    }
                });
            }

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
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greet_includes_the_name() {
        assert_eq!(greet("Ada"), "Hello, Ada! You've been greeted from Rust!");
    }
}
