//! First-run seeding of the default content (design spec §7).
//!
//! On the very first launch the store is empty; this module fills it with the
//! evidence-based movement rotation, the eight mobility drills, the loaded
//! strength session, and the default config. Presence of a config row is the
//! "already seeded" signal, so a re-launch never duplicates content and a user
//! who has since deleted a drill is never re-served it.

use thiserror::Error;

use crate::domain::{DomainError, Habit, Trigger};
use crate::store::{CalendarMode, Category, Config, NewRotation, Store, StoreError, WindowKind};

/// Errors raised while seeding. Wraps the store and domain errors so a failure
/// surfaces loudly rather than leaving a half-seeded database behind.
#[derive(Debug, Error)]
pub enum SeedError {
    #[error(transparent)]
    Store(#[from] StoreError),

    #[error(transparent)]
    Domain(#[from] DomainError),
}

/// The seed rotation fires every 30 minutes (design spec §7).
const ROTATION_INTERVAL_SECS: i64 = 30 * 60;

/// The seed rotation's name. Purely cosmetic — the rotation is identified by
/// its id everywhere that matters.
const ROTATION_NAME: &str = "Movement snacks";

/// The loaded strength session's weekly target (design spec §7).
const STRENGTH_WEEKLY_COUNT: u8 = 3;

/// One seed mobility drill: display name, instructions, and rotation weight.
struct SeedDrill {
    name: &'static str,
    instructions: &'static str,
    weight: u32,
}

/// The eight mobility drills (design spec §7). Lunge-and-reach is weighted 2;
/// the rest are weighted 1.
fn seed_drills() -> [SeedDrill; 8] {
    [
        SeedDrill {
            name: "Lunge-and-reach",
            instructions: "5 slow reps/leg, reach overhead",
            weight: 2,
        },
        SeedDrill {
            name: "Glute bridges",
            instructions: "20 reps, or single-leg 10/side",
            weight: 1,
        },
        SeedDrill {
            name: "Single-leg Romanian deadlift",
            instructions: "8/leg, 3s lower",
            weight: 1,
        },
        SeedDrill {
            name: "90/90 hip switches",
            instructions: "10 slow switches",
            weight: 1,
        },
        SeedDrill {
            name: "Half-kneeling hip-flexor stretch",
            instructions: "45s/side",
            weight: 1,
        },
        SeedDrill {
            name: "Light cardio snack",
            instructions: "3 min easy bike or step-ups",
            weight: 1,
        },
        SeedDrill {
            name: "Wall sit 40s → deep squat hold 45s",
            instructions: "Wall sit for 40s, then hold a deep squat for 45s",
            weight: 1,
        },
        SeedDrill {
            name: "Side-lying hip abduction",
            instructions: "15/side",
            weight: 1,
        },
    ]
}

/// The default config (design spec §7): rollover 04:00, day window
/// 09:00–18:00, calendar pause on in "with-others" mode, idle, DND, and the
/// microphone quiet rule on.
fn default_config() -> Config {
    Config {
        day_rollover: "04:00".to_string(),
        day_window_start: "09:00".to_string(),
        day_window_end: "18:00".to_string(),
        calendar_pause_enabled: true,
        calendar_mode: CalendarMode::WithOthers,
        idle_enabled: true,
        dnd_enabled: true,
        mic_pause_enabled: true,
        start_at_login: false,
    }
}

/// Seeds the default content on first run. Returns `true` when seeding ran and
/// `false` when it was skipped because the store was already seeded (a config
/// row exists). Idempotent: safe to call on every launch.
pub fn seed_if_empty(store: &Store, created_at: i64) -> Result<bool, SeedError> {
    if store.read_config()?.is_some() {
        return Ok(false);
    }

    store.write_config(&default_config())?;
    let rotation_id = seed_rotation(store)?;
    seed_drills_into(store, rotation_id, created_at)?;
    seed_strength_session(store, created_at)?;
    Ok(true)
}

/// Inserts the movement rotation, returning its id so the drills can be linked
/// to it.
fn seed_rotation(store: &Store) -> Result<i64, SeedError> {
    let id = store.insert_rotation(&NewRotation {
        name: ROTATION_NAME.to_string(),
        interval_secs: ROTATION_INTERVAL_SECS,
        window_kind: WindowKind::InheritGlobal,
        window_start: None,
        window_end: None,
    })?;
    Ok(id)
}

/// Inserts each mobility drill as an `exercise` rotation member of `rotation_id`.
fn seed_drills_into(store: &Store, rotation_id: i64, created_at: i64) -> Result<(), SeedError> {
    for drill in seed_drills() {
        let habit = Habit::new(
            drill.name.to_string(),
            drill.instructions.to_string(),
            None,
            Category::Exercise,
            true,
            Trigger::rotation_member(drill.weight)?,
        )?;
        store.insert_habit(&habit.to_new_habit(created_at, Some(rotation_id))?)?;
    }
    Ok(())
}

/// Inserts the loaded strength session as a 3×/week weekly-count habit. It
/// auto-chooses a day within the global window (no preferred time) and does not
/// expire uncompleted days — the week simply re-arms until the count is met
/// (design spec §4.2/§4.7 example D).
fn seed_strength_session(store: &Store, created_at: i64) -> Result<(), SeedError> {
    let habit = Habit::new(
        "Loaded strength session".to_string(),
        "Progression: bridges → hip thrusts; add a real RDL".to_string(),
        None,
        Category::Exercise,
        true,
        Trigger::weekly_count(STRENGTH_WEEKLY_COUNT, None, false)?,
    )?;
    store.insert_habit(&habit.to_new_habit(created_at, None)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{ScheduleTrigger, Trigger, WeeklyCountConfig};
    use crate::store::{Store, TriggerKind};

    const CREATED_AT: i64 = 1_700_000_000;

    fn seeded_store() -> Store {
        let store = Store::open_in_memory().expect("in-memory store opens");
        let ran = seed_if_empty(&store, CREATED_AT).expect("seeding succeeds");
        assert!(ran, "seeding ran on a fresh store");
        store
    }

    #[test]
    fn seeding_a_fresh_store_writes_the_default_config() {
        // Given a fresh store, when seeded
        let store = seeded_store();

        // Then the design-spec default config is written
        let config = store
            .read_config()
            .expect("read succeeds")
            .expect("config row present");
        assert_eq!(config.day_rollover, "04:00");
        assert_eq!(config.day_window_start, "09:00");
        assert_eq!(config.day_window_end, "18:00");
        assert_eq!(config.calendar_mode, CalendarMode::WithOthers);
        assert!(config.calendar_pause_enabled);
        assert!(config.idle_enabled);
        assert!(config.dnd_enabled);
        assert!(config.mic_pause_enabled);
        assert!(!config.start_at_login);
    }

    #[test]
    fn seeding_creates_one_rotation_every_30_minutes_inheriting_the_global_window() {
        // Given a fresh store, when seeded
        let store = seeded_store();

        // Then exactly one rotation exists, ticking every 30 minutes and
        // inheriting the global day window
        let rotations = store.list_rotations().expect("list succeeds");
        assert_eq!(rotations.len(), 1);
        assert_eq!(rotations[0].interval_secs, 1_800);
        assert_eq!(rotations[0].window_kind, WindowKind::InheritGlobal);
        assert_eq!(rotations[0].window_start, None);
        assert_eq!(rotations[0].window_end, None);
    }

    #[test]
    fn seeding_creates_the_eight_mobility_drills_all_exercise_members_of_the_rotation() {
        // Given a fresh store, when seeded
        let store = seeded_store();
        let rotation_id = store.list_rotations().expect("list succeeds")[0].id;

        // Then eight rotation-member drills exist, all `exercise`, all linked
        // to the seed rotation
        let drills: Vec<_> = store
            .list_habits()
            .expect("list succeeds")
            .into_iter()
            .filter(|habit| habit.trigger_kind == TriggerKind::RotationMember)
            .collect();
        assert_eq!(drills.len(), 8);
        for drill in &drills {
            assert_eq!(drill.category, Category::Exercise);
            assert_eq!(drill.rotation_id, Some(rotation_id));
            assert!(drill.enabled);
            assert_eq!(drill.created_at, CREATED_AT);
        }
    }

    #[test]
    fn seeding_weights_lunge_and_reach_two_and_every_other_drill_one() {
        // Given a fresh store, when seeded
        let store = seeded_store();

        // Then Lunge-and-reach carries weight 2 and the rest weight 1
        let habits = store.list_habits().expect("list succeeds");
        let lunge = habits
            .iter()
            .find(|habit| habit.name == "Lunge-and-reach")
            .expect("Lunge-and-reach seeded");
        assert_eq!(lunge.weight, Some(2));

        let others = habits
            .iter()
            .filter(|habit| {
                habit.trigger_kind == TriggerKind::RotationMember && habit.name != "Lunge-and-reach"
            })
            .count();
        assert_eq!(others, 7);
        for habit in &habits {
            if habit.trigger_kind == TriggerKind::RotationMember && habit.name != "Lunge-and-reach"
            {
                assert_eq!(habit.weight, Some(1));
            }
        }
    }

    #[test]
    fn seeding_creates_the_loaded_strength_session_as_a_three_times_weekly_count() {
        // Given a fresh store, when seeded
        let store = seeded_store();

        // Then a single weekly-count habit exists targeting three days a week
        let strength: Vec<_> = store
            .list_habits()
            .expect("list succeeds")
            .into_iter()
            .filter(|habit| habit.trigger_kind == TriggerKind::ScheduleWeeklyCount)
            .collect();
        assert_eq!(strength.len(), 1);
        assert_eq!(strength[0].name, "Loaded strength session");
        assert_eq!(strength[0].category, Category::Exercise);
        assert_eq!(strength[0].weight, None);

        // And its trigger decodes to a count of three with no preferred time
        let habit = Habit::from_store(&strength[0]).expect("reconstructs");
        assert_eq!(
            habit.trigger,
            Trigger::Schedule(ScheduleTrigger::WeeklyCount(WeeklyCountConfig {
                count: STRENGTH_WEEKLY_COUNT,
                preferred_time: None,
                expires_at_day_end: false,
            }))
        );
    }

    #[test]
    fn seeding_creates_nine_habits_in_total() {
        // Given a fresh store, when seeded
        let store = seeded_store();

        // Then eight drills plus one strength session are present
        assert_eq!(store.list_habits().expect("list succeeds").len(), 9);
    }

    #[test]
    fn seeding_is_idempotent_a_second_call_is_a_no_op() {
        // Given an already-seeded store
        let store = seeded_store();
        let habits_after_first = store.list_habits().expect("list succeeds").len();

        // When seeding is invoked again
        let ran_again = seed_if_empty(&store, CREATED_AT + 1).expect("second seed call succeeds");

        // Then it reports that it did nothing and adds no further content
        assert!(!ran_again);
        assert_eq!(
            store.list_habits().expect("list succeeds").len(),
            habits_after_first
        );
        assert_eq!(store.list_rotations().expect("list succeeds").len(), 1);
    }

    #[test]
    fn seeding_is_skipped_when_a_config_row_already_exists() {
        // Given a store whose config was written but which has no content —
        // e.g. a user who deleted every default drill
        let store = Store::open_in_memory().expect("in-memory store opens");
        store
            .write_config(&default_config())
            .expect("write succeeds");

        // When seeding is attempted
        let ran = seed_if_empty(&store, CREATED_AT).expect("seed call succeeds");

        // Then it does not run — deleted content is never resurrected
        assert!(!ran);
        assert!(store.list_habits().expect("list succeeds").is_empty());
    }
}
