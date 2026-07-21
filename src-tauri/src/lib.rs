// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/

// The store's public API isn't consumed outside its own tests yet — that
// lands with the `commands` task, which wires it up to the frontend.
#[allow(dead_code, unused_imports)]
mod store;

// The domain model's public API isn't consumed outside its own tests yet —
// that lands with the `scheduler` and `commands` tasks.
#[allow(dead_code, unused_imports)]
mod domain;

/// Smoke-test command wired through the IPC bridge to prove the Rust <-> UI
/// round trip works. Replaced by real habit commands once features land.
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! You've been greeted from Rust!")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet])
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
