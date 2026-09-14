//! The rmcp server (design spec §6): an in-process MCP server exposing the
//! habit tools over a **local transport only** (stdio or an in-memory duplex
//! — never a network socket). This is how a locally-running LLM manages the
//! app; nothing it does leaves the machine.
//!
//! This file is the only impure edge of the MCP surface: it owns the shared
//! [`Store`] handle, the async tool methods, and the mapping from the
//! rmcp-agnostic [`McpToolError`] onto the SDK's `ErrorData`. All actual work
//! is delegated to the pure handlers in `handlers.rs`.

use std::sync::{Arc, Mutex, MutexGuard};

use chrono::Utc;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_handler, tool_router, ErrorData, Json, ServerHandler};

use crate::store::{Store, StoreError};

use super::dto::{
    AddHabitRequest, AddHabitResponse, DayLogRequest, DayLogResponse, DisableHabitRequest,
    DisableHabitResponse, ListHabitsResponse, ListRotationsResponse, LogEventRequest,
    LogEventResponse, QueryLogRequest, QueryLogResponse, UpdateHabitRequest, UpdateHabitResponse,
};
use super::error::McpToolError;
use super::handlers;

/// Maps a handler error onto the SDK's `ErrorData`. Validation failures are
/// `invalid_params` (the caller sent something wrong); genuine database
/// faults are `internal_error`.
fn to_error_data(error: McpToolError) -> ErrorData {
    match error {
        McpToolError::Domain(_)
        | McpToolError::Store(StoreError::NotFound { .. })
        | McpToolError::InvalidDate(_)
        | McpToolError::InvalidRotation(_) => ErrorData::invalid_params(error.to_string(), None),
        McpToolError::Store(StoreError::Database(_)) | McpToolError::ConfigNotSet => {
            ErrorData::internal_error(error.to_string(), None)
        }
    }
}

/// The in-process MCP server. Holds the shared store behind a mutex so the
/// tool methods — which see only `&self` — can reach it; the guard is always
/// dropped before returning, so no lock is ever held across an await point.
#[derive(Clone)]
pub struct HabitsServer {
    store: Arc<Mutex<Store>>,
    tool_router: ToolRouter<Self>,
}

impl HabitsServer {
    pub fn new(store: Arc<Mutex<Store>>) -> Self {
        Self {
            store,
            tool_router: Self::tool_router(),
        }
    }

    fn lock_store(&self) -> Result<MutexGuard<'_, Store>, ErrorData> {
        self.store
            .lock()
            .map_err(|_| ErrorData::internal_error("the store mutex is poisoned", None))
    }
}

#[tool_router]
impl HabitsServer {
    #[tool(description = "Add a habit (content plus exactly one trigger) to the local store.")]
    async fn add_habit(
        &self,
        params: Parameters<AddHabitRequest>,
    ) -> Result<Json<AddHabitResponse>, ErrorData> {
        let created_at = Utc::now().timestamp();
        let store = self.lock_store()?;
        handlers::add_habit(&store, params.0, created_at)
            .map(Json)
            .map_err(to_error_data)
    }

    #[tool(description = "List every habit, enabled or disabled, in insertion order.")]
    async fn list_habits(&self) -> Result<Json<ListHabitsResponse>, ErrorData> {
        let store = self.lock_store()?;
        handlers::list_habits(&store)
            .map(Json)
            .map_err(to_error_data)
    }

    #[tool(
        description = "List every rotation with its members (id, name, weight), so a valid \
                        rotation_id can be discovered before adding a rotation-member habit."
    )]
    async fn list_rotations(&self) -> Result<Json<ListRotationsResponse>, ErrorData> {
        let store = self.lock_store()?;
        handlers::list_rotations(&store)
            .map(Json)
            .map_err(to_error_data)
    }

    #[tool(description = "Update a habit's content fields in place; omitted fields are unchanged.")]
    async fn update_habit(
        &self,
        params: Parameters<UpdateHabitRequest>,
    ) -> Result<Json<UpdateHabitResponse>, ErrorData> {
        let store = self.lock_store()?;
        handlers::update_habit(&store, params.0)
            .map(Json)
            .map_err(to_error_data)
    }

    #[tool(description = "Disable a habit: excluded from scheduling, its history retained.")]
    async fn disable_habit(
        &self,
        params: Parameters<DisableHabitRequest>,
    ) -> Result<Json<DisableHabitResponse>, ErrorData> {
        let store = self.lock_store()?;
        handlers::disable_habit(&store, params.0)
            .map(Json)
            .map_err(to_error_data)
    }

    #[tool(
        description = "Query the event log with optional habit, action and time-window filters."
    )]
    async fn query_log(
        &self,
        params: Parameters<QueryLogRequest>,
    ) -> Result<Json<QueryLogResponse>, ErrorData> {
        let store = self.lock_store()?;
        handlers::query_log(&store, params.0)
            .map(Json)
            .map_err(to_error_data)
    }

    #[tool(description = "Append an event (done/skipped/snoozed/expired) for a habit to the log.")]
    async fn log_event(
        &self,
        params: Parameters<LogEventRequest>,
    ) -> Result<Json<LogEventResponse>, ErrorData> {
        let store = self.lock_store()?;
        handlers::log_event(&store, params.0)
            .map(Json)
            .map_err(to_error_data)
    }

    #[tool(
        description = "Fetch a rollover-day's event log (date as YYYY-MM-DD) plus its day \
                        summary (done/skipped counts, moving time, adherence) and longest \
                        sedentary gap, so a locally-running LLM can read adherence."
    )]
    async fn day_log(
        &self,
        params: Parameters<DayLogRequest>,
    ) -> Result<Json<DayLogResponse>, ErrorData> {
        let now = Utc::now().naive_utc();
        let store = self.lock_store()?;
        handlers::day_log(&store, params.0, now)
            .map(Json)
            .map_err(to_error_data)
    }
}

#[tool_handler]
impl ServerHandler for HabitsServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Fully-local habits app. All tools operate on the on-device SQLite store; \
             nothing leaves the machine.",
        )
    }
}

/// Serves the MCP tools over stdio until the peer disconnects (design spec
/// §6 — local transport only). Intended to be launched by a locally-running
/// LLM that speaks MCP over the app's stdin/stdout.
pub async fn serve_stdio(store: Arc<Mutex<Store>>) -> Result<(), Box<dyn std::error::Error>> {
    use rmcp::transport::stdio;
    use rmcp::ServiceExt;

    let running = HabitsServer::new(store).serve(stdio()).await?;
    running.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::dto::{ActionDto, CategoryDto, DayLogRequest, TriggerDto};
    use crate::store::{CalendarMode, Config};
    use rmcp::model::CallToolRequestParams;
    use rmcp::serde_json::{self, Map, Value};
    use rmcp::ServiceExt;

    /// Serialises a request DTO into the flat JSON object arguments a tool
    /// call expects.
    fn arguments(request: impl serde::Serialize) -> Map<String, Value> {
        match serde_json::to_value(request).expect("request serialises") {
            Value::Object(map) => map,
            other => panic!("request did not serialise to an object: {other:?}"),
        }
    }

    /// Spins up the server and a client either side of an in-memory duplex —
    /// a genuinely local transport, no network involved — and returns the
    /// connected client peer.
    async fn connect() -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
        connect_with_store(Store::open_in_memory().expect("store opens")).await
    }

    /// As [`connect`], but over a caller-supplied store — lets a test
    /// pre-seed state (e.g. writing config) before the server sees it.
    async fn connect_with_store(
        store: Store,
    ) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
        let store = Arc::new(Mutex::new(store));
        let (server_transport, client_transport) = tokio::io::duplex(4096);
        let server = HabitsServer::new(store);
        tokio::spawn(async move {
            let running = server.serve(server_transport).await.expect("server starts");
            running.waiting().await.expect("server runs");
        });
        ().serve(client_transport).await.expect("client connects")
    }

    #[tokio::test]
    async fn the_server_advertises_exactly_the_eight_habit_tools() {
        // Given a connected client
        let client = connect().await;

        // When listing the server's tools
        let tools = client.list_all_tools().await.expect("list tools succeeds");

        // Then all eight tools from the design spec §6/§6.1 are present
        let mut names: Vec<String> = tools.iter().map(|tool| tool.name.to_string()).collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "add_habit",
                "day_log",
                "disable_habit",
                "list_habits",
                "list_rotations",
                "log_event",
                "query_log",
                "update_habit",
            ]
        );

        client.cancel().await.expect("client shuts down");
    }

    #[tokio::test]
    async fn list_rotations_returns_a_seeded_rotation_and_its_member_over_the_transport() {
        // Given a store pre-seeded with a rotation, connected over the transport
        let store = Store::open_in_memory().expect("store opens");
        let rotation_id = store
            .insert_rotation(&crate::store::NewRotation {
                name: "Movement snacks".to_string(),
                interval_secs: 1_800,
                window_kind: crate::store::WindowKind::InheritGlobal,
                window_start: None,
                window_end: None,
            })
            .expect("rotation insert succeeds");
        let client = connect_with_store(store).await;

        // And a rotation-member habit added over the wire, referencing that id
        let add = client
            .call_tool(
                CallToolRequestParams::new("add_habit").with_arguments(arguments(
                    AddHabitRequest {
                        name: "Lunge-and-reach".to_string(),
                        instructions: "5 slow reps/leg".to_string(),
                        media_path: None,
                        category: CategoryDto::Exercise,
                        enabled: true,
                        trigger: TriggerDto::RotationMember {
                            weight: 3,
                            rotation_id: Some(rotation_id),
                        },
                    },
                )),
            )
            .await
            .expect("add_habit call succeeds");
        let member_id = serde_json::from_value::<AddHabitResponse>(
            add.structured_content.expect("structured content present"),
        )
        .expect("response deserialises")
        .id;

        // When list_rotations is called over the transport
        let list = client
            .call_tool(CallToolRequestParams::new("list_rotations"))
            .await
            .expect("list_rotations call succeeds");
        let content = list.structured_content.expect("structured content present");

        // Then the window kind is rendered in kebab-case on the wire — the shape
        // a client actually parses; a broken `rename_all` would otherwise slip
        // through the typed round-trip below undetected
        assert_eq!(
            content["rotations"][0]["window_kind"],
            serde_json::json!("inherit-global"),
            "window_kind must be kebab-case on the wire"
        );

        let listed: ListRotationsResponse =
            serde_json::from_value(content).expect("response deserialises");

        // And the rotation comes back with its id and its member's weight, so a
        // caller could pick this rotation_id for a further add_habit
        assert_eq!(listed.rotations.len(), 1);
        assert_eq!(listed.rotations[0].id, rotation_id);
        assert_eq!(listed.rotations[0].members.len(), 1);
        assert_eq!(listed.rotations[0].members[0].habit_id, member_id);
        assert_eq!(listed.rotations[0].members[0].weight, Some(3));

        client.cancel().await.expect("client shuts down");
    }

    #[tokio::test]
    async fn add_habit_then_list_habits_round_trips_over_the_transport() {
        // Given a connected client
        let client = connect().await;

        // When a habit is added via the add_habit tool
        let add = client
            .call_tool(
                CallToolRequestParams::new("add_habit").with_arguments(arguments(
                    AddHabitRequest {
                        name: "Lunge-and-reach".to_string(),
                        instructions: "5 slow reps/leg, reach overhead".to_string(),
                        media_path: None,
                        category: CategoryDto::Exercise,
                        enabled: true,
                        trigger: TriggerDto::ScheduleWeeklyCount {
                            count: 3,
                            preferred_time: None,
                            expires_at_day_end: false,
                        },
                    },
                )),
            )
            .await
            .expect("add_habit call succeeds");
        let added: AddHabitResponse =
            serde_json::from_value(add.structured_content.expect("structured content present"))
                .expect("response deserialises");

        // And the habits are listed via the list_habits tool
        let list = client
            .call_tool(CallToolRequestParams::new("list_habits"))
            .await
            .expect("list_habits call succeeds");
        let listed: ListHabitsResponse =
            serde_json::from_value(list.structured_content.expect("structured content present"))
                .expect("response deserialises");

        // Then the added habit comes back with the same id and content
        assert_eq!(listed.habits.len(), 1);
        assert_eq!(listed.habits[0].id, added.id);
        assert_eq!(listed.habits[0].name, "Lunge-and-reach");
        assert_eq!(listed.habits[0].category, CategoryDto::Exercise);

        client.cancel().await.expect("client shuts down");
    }

    #[tokio::test]
    async fn log_event_then_query_log_round_trips_over_the_transport() {
        // Given a connected client with one habit
        let client = connect().await;
        let add = client
            .call_tool(
                CallToolRequestParams::new("add_habit").with_arguments(arguments(
                    AddHabitRequest {
                        name: "Glute bridges".to_string(),
                        instructions: "20, or single-leg 10/side".to_string(),
                        media_path: None,
                        category: CategoryDto::Exercise,
                        enabled: true,
                        trigger: TriggerDto::ScheduleWeeklyCount {
                            count: 3,
                            preferred_time: None,
                            expires_at_day_end: false,
                        },
                    },
                )),
            )
            .await
            .expect("add_habit call succeeds");
        let habit_id = serde_json::from_value::<AddHabitResponse>(
            add.structured_content.expect("structured content present"),
        )
        .expect("response deserialises")
        .id;

        // When a done event is logged, then queried back
        client
            .call_tool(
                CallToolRequestParams::new("log_event").with_arguments(arguments(
                    LogEventRequest {
                        habit_id,
                        action: ActionDto::Done,
                        at: 1_700_000_100,
                    },
                )),
            )
            .await
            .expect("log_event call succeeds");
        let query = client
            .call_tool(CallToolRequestParams::new("query_log"))
            .await
            .expect("query_log call succeeds");
        let log: QueryLogResponse = serde_json::from_value(
            query
                .structured_content
                .expect("structured content present"),
        )
        .expect("response deserialises");

        // Then exactly that event is returned
        assert_eq!(log.events.len(), 1);
        assert_eq!(log.events[0].habit_id, habit_id);
        assert_eq!(log.events[0].action, ActionDto::Done);

        client.cancel().await.expect("client shuts down");
    }

    #[tokio::test]
    async fn day_log_returns_the_days_events_plus_the_summary_over_the_transport() {
        // Given a configured store, connected over the local transport
        let store = Store::open_in_memory().expect("store opens");
        store
            .write_config(&Config {
                day_rollover: "04:00".to_string(),
                day_window_start: "09:00".to_string(),
                day_window_end: "18:00".to_string(),
                calendar_pause_enabled: true,
                calendar_mode: CalendarMode::WithOthers,
                idle_enabled: true,
                dnd_enabled: true,
                mic_pause_enabled: true,
                start_at_login: false,
            })
            .expect("write succeeds");
        let client = connect_with_store(store).await;

        // And a habit with a logged done event, added over the transport
        let add = client
            .call_tool(
                CallToolRequestParams::new("add_habit").with_arguments(arguments(
                    AddHabitRequest {
                        name: "Wall sit".to_string(),
                        instructions: "40s hold".to_string(),
                        media_path: None,
                        category: CategoryDto::Exercise,
                        enabled: true,
                        trigger: TriggerDto::ScheduleWeeklyCount {
                            count: 3,
                            preferred_time: None,
                            expires_at_day_end: false,
                        },
                    },
                )),
            )
            .await
            .expect("add_habit call succeeds");
        let habit_id = serde_json::from_value::<AddHabitResponse>(
            add.structured_content.expect("structured content present"),
        )
        .expect("response deserialises")
        .id;
        client
            .call_tool(
                CallToolRequestParams::new("log_event").with_arguments(arguments(
                    LogEventRequest {
                        habit_id,
                        action: ActionDto::Done,
                        at: 1_700_000_100,
                    },
                )),
            )
            .await
            .expect("log_event call succeeds");

        // When day_log is called for the date that event's timestamp falls on
        let logged_date = chrono::DateTime::from_timestamp(1_700_000_100, 0)
            .expect("valid timestamp")
            .format("%Y-%m-%d")
            .to_string();
        let response = client
            .call_tool(
                CallToolRequestParams::new("day_log").with_arguments(arguments(DayLogRequest {
                    date: logged_date.clone(),
                })),
            )
            .await
            .expect("day_log call succeeds");
        let log: DayLogResponse = serde_json::from_value(
            response
                .structured_content
                .expect("structured content present"),
        )
        .expect("response deserialises");

        // Then the day's log carries the event, joined with the habit's name,
        // and the day summary reflects one completed drill
        assert_eq!(log.date, logged_date);
        assert_eq!(log.events.len(), 1);
        assert_eq!(log.events[0].habit_name, "Wall sit");
        assert_eq!(log.summary.done_count, 1);
        assert_eq!(log.summary.adherence_pct, 100.0);

        client.cancel().await.expect("client shuts down");
    }

    #[tokio::test]
    async fn invalid_input_surfaces_a_loud_tool_error_rather_than_silent_success() {
        // Given a connected client
        let client = connect().await;

        // When add_habit is called with a blank name
        let result = client
            .call_tool(
                CallToolRequestParams::new("add_habit").with_arguments(arguments(
                    AddHabitRequest {
                        name: "   ".to_string(),
                        instructions: "instructions".to_string(),
                        media_path: None,
                        category: CategoryDto::General,
                        enabled: true,
                        trigger: TriggerDto::RotationMember {
                            weight: 1,
                            rotation_id: None,
                        },
                    },
                )),
            )
            .await;

        // Then the call surfaces a loud invalid-params error, not a silent
        // success, and nothing is stored
        let error = result.expect_err("invalid input must fail loudly");
        assert!(
            error.to_string().contains("must not be empty"),
            "unexpected error: {error}"
        );
        let list = client
            .call_tool(CallToolRequestParams::new("list_habits"))
            .await
            .expect("list_habits call succeeds");
        let listed: ListHabitsResponse =
            serde_json::from_value(list.structured_content.expect("structured content present"))
                .expect("response deserialises");
        assert!(listed.habits.is_empty());

        client.cancel().await.expect("client shuts down");
    }
}
