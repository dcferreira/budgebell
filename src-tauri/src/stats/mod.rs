//! The stats data path (design spec §6.1): pure aggregation over a day's
//! events — the day summary and the longest sedentary gap — plus the shared
//! date-ranged query that resolves a rollover-day's real time bounds and
//! fetches its events from the store.
//!
//! `gap` and `summary` are pure: no clock, no database, so they are
//! exhaustively unit-tested. `query` is the one impure edge in this module —
//! it reads the store — and is reused as-is by the `day_log` Tauri command
//! and the MCP `day_log` tool, so the resolution logic exists exactly once.

mod gap;
mod query;
mod summary;

pub use gap::{longest_sedentary_gap, SedentaryGap};
pub use query::{day_log, DayLog};
pub use summary::{day_summary, DaySummary};
