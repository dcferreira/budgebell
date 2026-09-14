//! End-to-end test of the headless MCP stdio server (design spec §6).
//!
//! Unlike the unit tests in `mcp::server` (which drive an in-memory duplex),
//! this spawns the **real built binary** with `HABITS_MCP_STDIO` set against a
//! throwaway database and talks to it through a genuine rmcp client over a
//! child-process transport — the real transport an MCP client uses. It proves
//! the previously-unexercised stdio path works end-to-end and that calls are
//! sequenced correctly (add then list reflects the add).

use rmcp::model::CallToolRequestParams;
use rmcp::serde_json::{self, Map, Value};
use rmcp::transport::TokioChildProcess;
use rmcp::ServiceExt;

/// Flattens a JSON object into the argument map a tool call expects.
fn arguments(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        other => panic!("arguments did not serialise to an object: {other:?}"),
    }
}

/// The `habits` array from a `list_habits` structured response.
fn listed_habits(content: Option<Value>) -> Vec<Value> {
    content
        .expect("structured content present")
        .get("habits")
        .and_then(Value::as_array)
        .cloned()
        .expect("a habits array")
}

#[tokio::test]
async fn the_headless_stdio_server_lists_and_adds_habits_over_a_child_process() {
    // Given the real binary in headless MCP mode against a throwaway DB
    let dir = tempfile::tempdir().expect("temp dir created");
    let db = dir.path().join("habits.sqlite");
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_habits"));
    command
        .env("HABITS_MCP_STDIO", "1")
        .env("HABITS_DB_PATH", &db);
    let client =
        ().serve(TokioChildProcess::new(command).expect("child process spawns"))
            .await
            .expect("client connects to the headless server");

    // Then a fresh database starts with no habits
    let before = client
        .call_tool(CallToolRequestParams::new("list_habits"))
        .await
        .expect("list_habits succeeds");
    assert!(
        listed_habits(before.structured_content).is_empty(),
        "a fresh DB has no habits"
    );

    // When a habit is added over the wire
    let added = client
        .call_tool(
            CallToolRequestParams::new("add_habit").with_arguments(arguments(serde_json::json!({
                "name": "E2E stretch",
                "instructions": "hold 30s",
                "category": "general",
                "enabled": true,
                "trigger": { "kind": "schedule-weekly-count", "count": 3 },
            }))),
        )
        .await
        .expect("add_habit succeeds");
    let new_id = added
        .structured_content
        .expect("structured content present")["id"]
        .as_i64()
        .expect("an integer id");

    // Then a subsequent (awaited, so un-raced) list reflects the add
    let after = client
        .call_tool(CallToolRequestParams::new("list_habits"))
        .await
        .expect("list_habits succeeds");
    let habits = listed_habits(after.structured_content);
    assert_eq!(habits.len(), 1, "the added habit is listed");
    assert_eq!(habits[0]["id"].as_i64().expect("an id"), new_id);
    assert_eq!(habits[0]["name"].as_str().expect("a name"), "E2E stretch");

    // And disconnecting closes the child's stdin; the server exits on its own
    // (the lifecycle this change fixes), which `cancel` awaits cleanly.
    client.cancel().await.expect("client shuts the server down");
}
