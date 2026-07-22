// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

mod commands;

// Large swathes of the store's and domain model's CRUD/construction surface
// (inserting/updating habits and rotations, listing events, building
// triggers from scratch) aren't reachable from the commands wired up so
// far — they're consumed by the `seed-wire` and `mcp` tasks still to come.
#[allow(dead_code, unused_imports)]
mod domain;
// The idle/DND/EventKit probes and their pure parsing helpers are exercised
// by the `list_due` command and the module's own unit tests; the broader
// helper surface is consumed as the app grows.
#[allow(dead_code, unused_imports)]
mod quiet_os;
mod scheduler;
#[allow(dead_code, unused_imports)]
mod store;
mod tray;

use tauri::Manager;

use commands::{
    complete_habit, get_config, list_due, list_habits, pause, resume, set_config, skip_habit,
    snooze_habit, AppState,
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
            let store = Store::open(app_data_dir.join("habits.sqlite")).expect("the store opens");
            app.manage(AppState::new(store));

            // The macOS menu-bar tray (design spec §3.3) — its menu events
            // drive the pause off-switch and open the app's windows.
            tray::setup_tray(app.handle()).expect("the tray icon is created");
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
            get_config,
            set_config,
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
