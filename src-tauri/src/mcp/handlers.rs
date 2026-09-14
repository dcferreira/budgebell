//! The MCP tool handlers (design spec §6), each a small function delegating
//! to the [`Store`]. They take `&Store` and typed request DTOs and return
//! typed responses, so they are exhaustively unit-testable against an
//! in-memory database with no rmcp SDK, transport, or async runtime involved.
//! The server boundary (`server.rs`) is the only impure edge.

use chrono::{NaiveDate, NaiveDateTime};

use crate::domain::DayConfig;
use crate::stats;
use crate::store::{NewEvent, NewHabit, Store, TriggerKind};

use super::dto::{
    AddHabitRequest, AddHabitResponse, DayLogRequest, DayLogResponse, DisableHabitRequest,
    DisableHabitResponse, EventDto, HabitDto, ListHabitsResponse, ListRotationsResponse,
    LogEventRequest, LogEventResponse, QueryLogRequest, QueryLogResponse, RotationDto,
    RotationMemberDto, UpdateHabitRequest, UpdateHabitResponse,
};
use super::error::McpToolError;

/// Adds a habit, validating it through the domain model before inserting.
/// `created_at` is injected (never read from the clock here) to keep the
/// handler pure and deterministic under test.
pub fn add_habit(
    store: &Store,
    request: AddHabitRequest,
    created_at: i64,
) -> Result<AddHabitResponse, McpToolError> {
    let new_habit = request.into_new_habit(created_at)?;
    validate_rotation_membership(store, &new_habit)?;
    let id = store.insert_habit(&new_habit)?;
    Ok(AddHabitResponse { id })
}

/// A rotation-member habit must belong to an existing rotation, or the
/// scheduler cannot place it and every tick fails (design spec §4.2/§7). MCP
/// callers can only ever reference an existing rotation — there is no
/// create-rotation tool — so a missing or unknown `rotation_id` is rejected
/// here rather than stored as an un-schedulable orphan.
fn validate_rotation_membership(store: &Store, habit: &NewHabit) -> Result<(), McpToolError> {
    if habit.trigger_kind != TriggerKind::RotationMember {
        return Ok(());
    }
    let rotation_id = habit.rotation_id.ok_or_else(|| {
        McpToolError::InvalidRotation(
            "a rotation-member habit must reference a rotation via rotation_id".to_string(),
        )
    })?;
    let exists = store
        .list_rotations()?
        .iter()
        .any(|rotation| rotation.id == rotation_id);
    if !exists {
        return Err(McpToolError::InvalidRotation(format!(
            "no rotation with id {rotation_id} exists"
        )));
    }
    Ok(())
}

/// Lists every habit — enabled or disabled — in insertion order.
pub fn list_habits(store: &Store) -> Result<ListHabitsResponse, McpToolError> {
    let habits = store
        .list_habits()?
        .into_iter()
        .map(HabitDto::from)
        .collect();
    Ok(ListHabitsResponse { habits })
}

/// Lists every rotation with its members (design spec §4.3). MCP callers can
/// only reference an existing rotation — there is no create-rotation tool and
/// `add_habit` rejects an unknown `rotation_id` — so this is the caller's only
/// way to discover a valid id (and see what the rotation already holds) before
/// adding a rotation-member habit.
pub fn list_rotations(store: &Store) -> Result<ListRotationsResponse, McpToolError> {
    let habits = store.list_habits()?;
    let rotations = store
        .list_rotations()?
        .into_iter()
        .map(|rotation| {
            let members = habits
                .iter()
                .filter(|habit| habit.rotation_id == Some(rotation.id))
                .map(|habit| RotationMemberDto {
                    habit_id: habit.id,
                    name: habit.name.clone(),
                    weight: habit.weight,
                    enabled: habit.enabled,
                })
                .collect();
            RotationDto {
                id: rotation.id,
                name: rotation.name,
                interval_secs: rotation.interval_secs,
                window_kind: rotation.window_kind.into(),
                window_start: rotation.window_start,
                window_end: rotation.window_end,
                members,
            }
        })
        .collect();
    Ok(ListRotationsResponse { rotations })
}

/// Updates a habit's content fields in place. Omitted fields keep their
/// stored value; an unknown id fails loudly with `NotFound`.
pub fn update_habit(
    store: &Store,
    request: UpdateHabitRequest,
) -> Result<UpdateHabitResponse, McpToolError> {
    let mut habit = store
        .list_habits()?
        .into_iter()
        .find(|habit| habit.id == request.id)
        .ok_or(crate::store::StoreError::NotFound { id: request.id })?;

    if let Some(name) = request.name {
        habit.name = name;
    }
    if let Some(instructions) = request.instructions {
        habit.instructions = instructions;
    }
    if let Some(media_path) = request.media_path {
        habit.media_path = Some(media_path);
    }
    if let Some(category) = request.category {
        habit.category = category.into();
    }
    if let Some(enabled) = request.enabled {
        habit.enabled = enabled;
    }

    store.update_habit(&habit)?;
    Ok(UpdateHabitResponse { id: habit.id })
}

/// Disables a habit — excluded from scheduling, history retained.
pub fn disable_habit(
    store: &Store,
    request: DisableHabitRequest,
) -> Result<DisableHabitResponse, McpToolError> {
    store.disable_habit(request.id)?;
    Ok(DisableHabitResponse { id: request.id })
}

/// Queries the event log with optional AND-combined filters, returning the
/// most recent `limit` matches in chronological (oldest-first) order.
pub fn query_log(
    store: &Store,
    request: QueryLogRequest,
) -> Result<QueryLogResponse, McpToolError> {
    let action = request.action.map(Into::into);
    let mut events: Vec<EventDto> = store
        .list_events()?
        .into_iter()
        .filter(|event| request.habit_id.is_none_or(|id| event.habit_id == id))
        .filter(|event| action.is_none_or(|wanted| event.action == wanted))
        .filter(|event| request.since.is_none_or(|since| event.at >= since))
        .filter(|event| request.until.is_none_or(|until| event.at < until))
        .map(EventDto::from)
        .collect();

    // `list_events` is already chronological; keeping only the most recent
    // `limit` means dropping from the front.
    if let Some(limit) = request.limit {
        if events.len() > limit {
            events.drain(0..events.len() - limit);
        }
    }

    Ok(QueryLogResponse { events })
}

/// Fetches a rollover-day's event log plus its day summary and longest
/// sedentary gap (design spec §6.1), so a locally-running LLM can read
/// adherence — nothing this reaches for leaves the machine. `now` is
/// injected (never read from the clock here) to keep the handler
/// deterministic under test.
pub fn day_log(
    store: &Store,
    request: DayLogRequest,
    now: NaiveDateTime,
) -> Result<DayLogResponse, McpToolError> {
    let date = NaiveDate::parse_from_str(&request.date, "%Y-%m-%d")
        .map_err(|_| McpToolError::InvalidDate(request.date.clone()))?;
    let config = store.read_config()?.ok_or(McpToolError::ConfigNotSet)?;
    let day_config = DayConfig::try_from(&config)?;
    let log = stats::day_log(store, day_config, date, now)?;
    Ok(DayLogResponse::from(log))
}

/// Appends an event to the log (design spec §6 — optional `log_event`).
pub fn log_event(
    store: &Store,
    request: LogEventRequest,
) -> Result<LogEventResponse, McpToolError> {
    let id = store.append_event(&NewEvent {
        habit_id: request.habit_id,
        action: request.action.into(),
        at: request.at,
        shown_at: None,
    })?;
    Ok(LogEventResponse { id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::dto::{
        ActionDto, CategoryDto, RecurrenceDto, TriggerDto, WeekdayDto, WindowKindDto,
    };
    use crate::store::{Category, EventAction, NewRotation, Store, TriggerKind, WindowKind};

    const CREATED_AT: i64 = 1_700_000_000;

    /// A valid `add_habit` request; tests override only the fields they care
    /// about via struct-update syntax. The default trigger is a schedule (which
    /// needs no rotation), so tests that only need *a* habit stay valid without
    /// seeding a rotation; the rotation-member tests set their own trigger.
    fn sample_add_request() -> AddHabitRequest {
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
        }
    }

    fn seed_rotation(store: &Store) -> i64 {
        store
            .insert_rotation(&NewRotation {
                name: "Movement snacks".to_string(),
                interval_secs: 1_800,
                window_kind: WindowKind::InheritGlobal,
                window_start: None,
                window_end: None,
            })
            .expect("rotation insert succeeds")
    }

    #[test]
    fn add_habit_inserts_a_valid_rotation_member_and_returns_its_id() {
        // Given a store with a rotation and a valid rotation-member request
        let store = Store::open_in_memory().expect("store opens");
        let rotation_id = seed_rotation(&store);
        let request = AddHabitRequest {
            trigger: TriggerDto::RotationMember {
                weight: 2,
                rotation_id: Some(rotation_id),
            },
            ..sample_add_request()
        };

        // When add_habit is invoked
        let response = add_habit(&store, request, CREATED_AT).expect("add succeeds");

        // Then the habit is persisted with the returned id and the sent content
        let habits = store.list_habits().expect("list succeeds");
        assert_eq!(habits.len(), 1);
        assert_eq!(habits[0].id, response.id);
        assert_eq!(habits[0].name, "Lunge-and-reach");
        assert_eq!(habits[0].category, Category::Exercise);
        assert_eq!(habits[0].trigger_kind, TriggerKind::RotationMember);
        assert_eq!(habits[0].weight, Some(2));
        assert_eq!(habits[0].created_at, CREATED_AT);
    }

    #[test]
    fn add_habit_rejects_a_rotation_member_without_a_rotation_id() {
        // Given a rotation-member request that names no rotation
        let store = Store::open_in_memory().expect("store opens");
        let request = AddHabitRequest {
            trigger: TriggerDto::RotationMember {
                weight: 1,
                rotation_id: None,
            },
            ..sample_add_request()
        };

        // When add_habit is invoked
        let result = add_habit(&store, request, CREATED_AT);

        // Then it is rejected loudly and nothing is stored — an orphan
        // rotation member would otherwise break every scheduler tick
        assert!(matches!(result, Err(McpToolError::InvalidRotation(_))));
        assert!(store.list_habits().expect("list succeeds").is_empty());
    }

    #[test]
    fn add_habit_rejects_a_rotation_member_with_an_unknown_rotation_id() {
        // Given a rotation-member request naming a rotation that doesn't exist
        let store = Store::open_in_memory().expect("store opens");
        let request = AddHabitRequest {
            trigger: TriggerDto::RotationMember {
                weight: 1,
                rotation_id: Some(999),
            },
            ..sample_add_request()
        };

        // When add_habit is invoked
        let result = add_habit(&store, request, CREATED_AT);

        // Then it is rejected loudly and nothing is stored
        assert!(matches!(result, Err(McpToolError::InvalidRotation(_))));
        assert!(store.list_habits().expect("list succeeds").is_empty());
    }

    #[test]
    fn add_habit_links_a_rotation_member_to_its_rotation() {
        // Given a store with a rotation
        let store = Store::open_in_memory().expect("store opens");
        let rotation_id = seed_rotation(&store);
        let request = AddHabitRequest {
            trigger: TriggerDto::RotationMember {
                weight: 1,
                rotation_id: Some(rotation_id),
            },
            ..sample_add_request()
        };

        // When the habit is added as a member of that rotation
        add_habit(&store, request, CREATED_AT).expect("add succeeds");

        // Then the stored row links back to the rotation
        let habits = store.list_habits().expect("list succeeds");
        assert_eq!(habits[0].rotation_id, Some(rotation_id));
    }

    #[test]
    fn list_rotations_on_an_empty_store_returns_no_rotations() {
        // Given a store with no rotations
        let store = Store::open_in_memory().expect("store opens");

        // When list_rotations is invoked
        let response = list_rotations(&store).expect("list succeeds");

        // Then it returns an empty list rather than failing
        assert!(response.rotations.is_empty());
    }

    #[test]
    fn list_rotations_returns_each_rotation_with_its_metadata() {
        // Given two rotations: one inheriting the global window, one with its own
        let store = Store::open_in_memory().expect("store opens");
        let inherit_id = store
            .insert_rotation(&NewRotation {
                name: "Movement snacks".to_string(),
                interval_secs: 1_800,
                window_kind: WindowKind::InheritGlobal,
                window_start: None,
                window_end: None,
            })
            .expect("insert succeeds");
        let own_id = store
            .insert_rotation(&NewRotation {
                name: "Early birds".to_string(),
                interval_secs: 600,
                window_kind: WindowKind::Own,
                window_start: Some("07:00".to_string()),
                window_end: Some("08:30".to_string()),
            })
            .expect("insert succeeds");

        // When list_rotations is invoked
        let response = list_rotations(&store).expect("list succeeds");

        // Then both come back in insertion order, each with its metadata and no
        // members yet
        assert_eq!(response.rotations.len(), 2);
        let inherit = &response.rotations[0];
        assert_eq!(inherit.id, inherit_id);
        assert_eq!(inherit.name, "Movement snacks");
        assert_eq!(inherit.interval_secs, 1_800);
        assert_eq!(inherit.window_kind, WindowKindDto::InheritGlobal);
        assert_eq!(inherit.window_start, None);
        assert_eq!(inherit.window_end, None);
        assert!(inherit.members.is_empty());

        let own = &response.rotations[1];
        assert_eq!(own.id, own_id);
        assert_eq!(own.window_kind, WindowKindDto::Own);
        assert_eq!(own.window_start, Some("07:00".to_string()));
        assert_eq!(own.window_end, Some("08:30".to_string()));
    }

    #[test]
    fn list_rotations_attaches_members_with_their_weights_grouped_by_rotation() {
        // Given two rotations, each with a weighted member, plus a scheduled
        // habit that belongs to no rotation
        let store = Store::open_in_memory().expect("store opens");
        let first_id = seed_rotation(&store);
        let second_id = store
            .insert_rotation(&NewRotation {
                name: "Desk resets".to_string(),
                interval_secs: 3_600,
                window_kind: WindowKind::AlwaysOn,
                window_start: None,
                window_end: None,
            })
            .expect("insert succeeds");
        let first_member = add_habit(
            &store,
            AddHabitRequest {
                name: "Lunge-and-reach".to_string(),
                trigger: TriggerDto::RotationMember {
                    weight: 2,
                    rotation_id: Some(first_id),
                },
                ..sample_add_request()
            },
            CREATED_AT,
        )
        .expect("add succeeds")
        .id;
        let second_member = add_habit(
            &store,
            AddHabitRequest {
                name: "Neck rolls".to_string(),
                trigger: TriggerDto::RotationMember {
                    weight: 5,
                    rotation_id: Some(second_id),
                },
                ..sample_add_request()
            },
            CREATED_AT,
        )
        .expect("add succeeds")
        .id;
        add_habit(&store, sample_add_request(), CREATED_AT).expect("add succeeds");

        // When list_rotations is invoked
        let response = list_rotations(&store).expect("list succeeds");

        // Then each rotation carries only its own member, with the stored weight,
        // and the scheduled habit is attached to neither
        assert_eq!(response.rotations.len(), 2);
        assert_eq!(response.rotations[0].id, first_id);
        assert_eq!(response.rotations[0].members.len(), 1);
        assert_eq!(response.rotations[0].members[0].habit_id, first_member);
        assert_eq!(response.rotations[0].members[0].name, "Lunge-and-reach");
        assert_eq!(response.rotations[0].members[0].weight, Some(2));
        assert!(response.rotations[0].members[0].enabled);

        assert_eq!(response.rotations[1].id, second_id);
        assert_eq!(response.rotations[1].members.len(), 1);
        assert_eq!(response.rotations[1].members[0].habit_id, second_member);
        assert_eq!(response.rotations[1].members[0].weight, Some(5));
    }

    #[test]
    fn list_rotations_surfaces_disabled_members_with_their_enabled_flag() {
        // Given a rotation whose sole member is disabled — list_habits keeps
        // disabled habits, so a rotation-member habit stays visible to a caller
        // deciding what the rotation already holds
        let store = Store::open_in_memory().expect("store opens");
        let rotation_id = seed_rotation(&store);
        add_habit(
            &store,
            AddHabitRequest {
                enabled: false,
                trigger: TriggerDto::RotationMember {
                    weight: 1,
                    rotation_id: Some(rotation_id),
                },
                ..sample_add_request()
            },
            CREATED_AT,
        )
        .expect("add succeeds");

        // When list_rotations is invoked
        let response = list_rotations(&store).expect("list succeeds");

        // Then the member is still surfaced, carrying its disabled flag rather
        // than being dropped or silently reported as enabled
        assert_eq!(response.rotations[0].members.len(), 1);
        assert!(!response.rotations[0].members[0].enabled);
    }

    #[test]
    fn add_habit_persists_a_scheduled_at_time_trigger() {
        // Given a "09:00 daily, expires at day end" scheduled request
        let store = Store::open_in_memory().expect("store opens");
        let request = AddHabitRequest {
            category: CategoryDto::General,
            trigger: TriggerDto::ScheduleAtTime {
                time: "09:00".to_string(),
                recurrence: RecurrenceDto::Daily,
                expires_at_day_end: true,
            },
            ..sample_add_request()
        };

        // When it is added
        add_habit(&store, request, CREATED_AT).expect("add succeeds");

        // Then it is stored as a schedule-at-time habit with no rotation weight
        let habits = store.list_habits().expect("list succeeds");
        assert_eq!(habits[0].trigger_kind, TriggerKind::ScheduleAtTime);
        assert_eq!(habits[0].weight, None);
        assert!(habits[0].trigger_config_json.contains("daily"));
    }

    #[test]
    fn add_habit_persists_a_weekly_count_trigger_with_specific_weekdays_unused() {
        // Given a "3x / week at 17:00" weekly-count request (design spec §7)
        let store = Store::open_in_memory().expect("store opens");
        let request = AddHabitRequest {
            category: CategoryDto::General,
            trigger: TriggerDto::ScheduleWeeklyCount {
                count: 3,
                preferred_time: Some("17:00".to_string()),
                expires_at_day_end: false,
            },
            ..sample_add_request()
        };

        // When it is added
        add_habit(&store, request, CREATED_AT).expect("add succeeds");

        // Then it is stored as a weekly-count habit
        let habits = store.list_habits().expect("list succeeds");
        assert_eq!(habits[0].trigger_kind, TriggerKind::ScheduleWeeklyCount);
    }

    #[test]
    fn add_habit_maps_specific_weekdays_recurrence_through_to_the_domain() {
        // Given a "specific weekdays" schedule for Monday and Thursday
        let store = Store::open_in_memory().expect("store opens");
        let request = AddHabitRequest {
            trigger: TriggerDto::ScheduleAtTime {
                time: "07:00".to_string(),
                recurrence: RecurrenceDto::SpecificWeekdays {
                    days: vec![WeekdayDto::Monday, WeekdayDto::Thursday],
                },
                expires_at_day_end: false,
            },
            ..sample_add_request()
        };

        // When it is added
        add_habit(&store, request, CREATED_AT).expect("add succeeds");

        // Then the persisted config names both weekdays
        let habits = store.list_habits().expect("list succeeds");
        assert!(habits[0].trigger_config_json.contains("monday"));
        assert!(habits[0].trigger_config_json.contains("thursday"));
    }

    #[test]
    fn add_habit_rejects_an_empty_name_loudly() {
        // Given a request with a blank name
        let store = Store::open_in_memory().expect("store opens");
        let request = AddHabitRequest {
            name: "   ".to_string(),
            ..sample_add_request()
        };

        // When add_habit is invoked
        let result = add_habit(&store, request, CREATED_AT);

        // Then it fails with a domain validation error and nothing is stored
        assert!(matches!(result, Err(McpToolError::Domain(_))));
        assert!(store.list_habits().expect("list succeeds").is_empty());
    }

    #[test]
    fn add_habit_rejects_a_zero_weight_rotation_member() {
        // Given a rotation member with weight zero
        let store = Store::open_in_memory().expect("store opens");
        let request = AddHabitRequest {
            trigger: TriggerDto::RotationMember {
                weight: 0,
                rotation_id: None,
            },
            ..sample_add_request()
        };

        // When add_habit is invoked
        let result = add_habit(&store, request, CREATED_AT);

        // Then it fails loudly rather than storing a useless weight
        assert!(matches!(result, Err(McpToolError::Domain(_))));
    }

    #[test]
    fn add_habit_rejects_a_weekly_count_out_of_range() {
        // Given a weekly count above seven
        let store = Store::open_in_memory().expect("store opens");
        let request = AddHabitRequest {
            trigger: TriggerDto::ScheduleWeeklyCount {
                count: 8,
                preferred_time: None,
                expires_at_day_end: false,
            },
            ..sample_add_request()
        };

        // When add_habit is invoked
        let result = add_habit(&store, request, CREATED_AT);

        // Then it fails loudly
        assert!(matches!(result, Err(McpToolError::Domain(_))));
    }

    #[test]
    fn add_habit_rejects_an_unparsable_time() {
        // Given a scheduled trigger with a malformed time string
        let store = Store::open_in_memory().expect("store opens");
        let request = AddHabitRequest {
            trigger: TriggerDto::ScheduleAtTime {
                time: "0900".to_string(),
                recurrence: RecurrenceDto::Daily,
                expires_at_day_end: false,
            },
            ..sample_add_request()
        };

        // When add_habit is invoked
        let result = add_habit(&store, request, CREATED_AT);

        // Then it fails loudly rather than guessing the time
        assert!(matches!(result, Err(McpToolError::Domain(_))));
    }

    #[test]
    fn list_habits_returns_every_habit_in_insertion_order() {
        // Given two habits added in a known order
        let store = Store::open_in_memory().expect("store opens");
        add_habit(&store, sample_add_request(), CREATED_AT).expect("add succeeds");
        add_habit(
            &store,
            AddHabitRequest {
                name: "Glute bridges".to_string(),
                ..sample_add_request()
            },
            CREATED_AT,
        )
        .expect("add succeeds");

        // When list_habits is invoked
        let response = list_habits(&store).expect("list succeeds");

        // Then both come back in insertion order
        assert_eq!(response.habits.len(), 2);
        assert_eq!(response.habits[0].name, "Lunge-and-reach");
        assert_eq!(response.habits[1].name, "Glute bridges");
        assert_eq!(response.habits[0].category, CategoryDto::Exercise);
    }

    #[test]
    fn update_habit_changes_only_the_provided_fields() {
        // Given a stored habit
        let store = Store::open_in_memory().expect("store opens");
        let id = add_habit(&store, sample_add_request(), CREATED_AT)
            .expect("add succeeds")
            .id;

        // When only its name and category are updated
        let response = update_habit(
            &store,
            UpdateHabitRequest {
                id,
                name: Some("Renamed drill".to_string()),
                instructions: None,
                media_path: None,
                category: Some(CategoryDto::General),
                enabled: None,
            },
        )
        .expect("update succeeds");

        // Then the changed fields differ and the rest are untouched
        assert_eq!(response.id, id);
        let habit = store.list_habits().expect("list succeeds").remove(0);
        assert_eq!(habit.name, "Renamed drill");
        assert_eq!(habit.category, Category::General);
        assert_eq!(habit.instructions, "5 slow reps/leg, reach overhead");
        assert!(habit.enabled);
    }

    #[test]
    fn update_habit_fails_loudly_for_an_unknown_id() {
        // Given a store with no habits
        let store = Store::open_in_memory().expect("store opens");

        // When updating a habit id that was never added
        let result = update_habit(
            &store,
            UpdateHabitRequest {
                id: 999,
                name: Some("Ghost".to_string()),
                instructions: None,
                media_path: None,
                category: None,
                enabled: None,
            },
        );

        // Then it reports NotFound rather than silently succeeding
        assert!(matches!(
            result,
            Err(McpToolError::Store(crate::store::StoreError::NotFound {
                id: 999
            }))
        ));
    }

    #[test]
    fn disable_habit_flips_enabled_off_without_deleting() {
        // Given an enabled habit
        let store = Store::open_in_memory().expect("store opens");
        let id = add_habit(&store, sample_add_request(), CREATED_AT)
            .expect("add succeeds")
            .id;

        // When it is disabled
        let response = disable_habit(&store, DisableHabitRequest { id }).expect("disable succeeds");

        // Then it still exists but is no longer enabled
        assert_eq!(response.id, id);
        let habit = store.list_habits().expect("list succeeds").remove(0);
        assert!(!habit.enabled);
    }

    #[test]
    fn disable_habit_fails_loudly_for_an_unknown_id() {
        // Given a store with no habits
        let store = Store::open_in_memory().expect("store opens");

        // When disabling an id that was never added
        let result = disable_habit(&store, DisableHabitRequest { id: 42 });

        // Then it reports NotFound
        assert!(matches!(
            result,
            Err(McpToolError::Store(crate::store::StoreError::NotFound {
                id: 42
            }))
        ));
    }

    #[test]
    fn log_event_appends_an_event_and_query_log_reads_it_back() {
        // Given a habit to attach events to
        let store = Store::open_in_memory().expect("store opens");
        let habit_id = add_habit(&store, sample_add_request(), CREATED_AT)
            .expect("add succeeds")
            .id;

        // When a "done" event is logged
        log_event(
            &store,
            LogEventRequest {
                habit_id,
                action: ActionDto::Done,
                at: 1_700_000_100,
            },
        )
        .expect("log succeeds");

        // Then query_log returns exactly that event
        let response = query_log(&store, QueryLogRequest::default()).expect("query succeeds");
        assert_eq!(response.events.len(), 1);
        assert_eq!(response.events[0].habit_id, habit_id);
        assert_eq!(response.events[0].action, ActionDto::Done);
        assert_eq!(response.events[0].at, 1_700_000_100);
    }

    #[test]
    fn query_log_filters_by_habit_action_and_time_window() {
        // Given two habits with a mix of events at different times
        let store = Store::open_in_memory().expect("store opens");
        let first = add_habit(&store, sample_add_request(), CREATED_AT)
            .expect("add succeeds")
            .id;
        let second = add_habit(
            &store,
            AddHabitRequest {
                name: "Glute bridges".to_string(),
                ..sample_add_request()
            },
            CREATED_AT,
        )
        .expect("add succeeds")
        .id;
        for (habit_id, action, at) in [
            (first, EventAction::Done, 100),
            (first, EventAction::Skipped, 200),
            (second, EventAction::Done, 300),
        ] {
            store
                .append_event(&NewEvent {
                    habit_id,
                    action,
                    at,
                    shown_at: None,
                })
                .expect("append succeeds");
        }

        // When filtering to the first habit's "done" events within [50, 250)
        let response = query_log(
            &store,
            QueryLogRequest {
                habit_id: Some(first),
                action: Some(ActionDto::Done),
                since: Some(50),
                until: Some(250),
                limit: None,
            },
        )
        .expect("query succeeds");

        // Then only the matching event survives every filter
        assert_eq!(response.events.len(), 1);
        assert_eq!(response.events[0].habit_id, first);
        assert_eq!(response.events[0].at, 100);
    }

    fn dt(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(year, month, day)
            .expect("valid date")
            .and_hms_opt(hour, minute, 0)
            .expect("valid time")
    }

    fn default_config() -> crate::store::Config {
        crate::store::Config {
            day_rollover: "04:00".to_string(),
            day_window_start: "09:00".to_string(),
            day_window_end: "18:00".to_string(),
            calendar_pause_enabled: true,
            calendar_mode: crate::store::CalendarMode::WithOthers,
            idle_enabled: true,
            dnd_enabled: true,
            mic_pause_enabled: true,
            start_at_login: false,
        }
    }

    #[test]
    fn day_log_returns_the_rollover_days_events_joined_with_habit_details() {
        // Given a configured store with a habit and a done event on 2026-07-21
        let store = Store::open_in_memory().expect("store opens");
        store
            .write_config(&default_config())
            .expect("write succeeds");
        let habit_id = add_habit(&store, sample_add_request(), CREATED_AT)
            .expect("add succeeds")
            .id;
        log_event(
            &store,
            LogEventRequest {
                habit_id,
                action: ActionDto::Done,
                at: dt(2026, 7, 21, 10, 0).and_utc().timestamp(),
            },
        )
        .expect("log succeeds");

        // When fetching that day's log
        let response = day_log(
            &store,
            DayLogRequest {
                date: "2026-07-21".to_string(),
            },
            dt(2026, 7, 21, 14, 0),
        )
        .expect("day_log succeeds");

        // Then the event comes back joined with its habit's name and category,
        // and the summary reflects it
        assert_eq!(response.date, "2026-07-21");
        assert_eq!(response.events.len(), 1);
        assert_eq!(response.events[0].habit_name, "Lunge-and-reach");
        assert_eq!(response.events[0].category, CategoryDto::Exercise);
        assert_eq!(response.summary.done_count, 1);
        assert_eq!(response.summary.adherence_pct, 100.0);
    }

    #[test]
    fn day_log_rejects_an_unparsable_date_loudly() {
        // Given a configured store
        let store = Store::open_in_memory().expect("store opens");
        store
            .write_config(&default_config())
            .expect("write succeeds");

        // When day_log is called with a malformed date
        let result = day_log(
            &store,
            DayLogRequest {
                date: "21-07-2026".to_string(),
            },
            dt(2026, 7, 21, 14, 0),
        );

        // Then it fails loudly rather than guessing the date
        assert!(matches!(result, Err(McpToolError::InvalidDate(_))));
    }

    #[test]
    fn day_log_fails_loudly_when_config_has_never_been_written() {
        // Given a fresh store with no config row
        let store = Store::open_in_memory().expect("store opens");

        // When day_log is called
        let result = day_log(
            &store,
            DayLogRequest {
                date: "2026-07-21".to_string(),
            },
            dt(2026, 7, 21, 14, 0),
        );

        // Then it fails loudly rather than assuming a default day config
        assert!(matches!(result, Err(McpToolError::ConfigNotSet)));
    }

    #[test]
    fn query_log_limit_keeps_the_most_recent_events_in_chronological_order() {
        // Given three events at increasing times
        let store = Store::open_in_memory().expect("store opens");
        let habit_id = add_habit(&store, sample_add_request(), CREATED_AT)
            .expect("add succeeds")
            .id;
        for at in [100, 200, 300] {
            store
                .append_event(&NewEvent {
                    habit_id,
                    action: EventAction::Done,
                    at,
                    shown_at: None,
                })
                .expect("append succeeds");
        }

        // When querying with a limit of two
        let response = query_log(
            &store,
            QueryLogRequest {
                limit: Some(2),
                ..QueryLogRequest::default()
            },
        )
        .expect("query succeeds");

        // Then the two most recent events are returned, oldest-first
        assert_eq!(
            response
                .events
                .iter()
                .map(|event| event.at)
                .collect::<Vec<_>>(),
            vec![200, 300]
        );
    }
}
