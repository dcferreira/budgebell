// Frontend-side mirrors of src-tauri's IPC-shaped DTOs (see
// src-tauri/src/commands/dto.rs). Kept in sync by hand: field names and enum
// spellings must match the Rust `serde` output exactly.

/** The two content categories a habit can belong to (design spec §4.1). */
export type Category = "exercise" | "general";

/** A habit due right now, shaped for IPC — mirrors `DueHabitDto`. */
export interface DueHabit {
  habit_id: number;
  name: string;
  instructions: string;
  media_path: string | null;
  category: Category;
}

/**
 * A due habit as shown in the expanded dialog (design spec §3.2), which adds
 * an optional meta line (reps/duration) beneath the full instructions. This
 * field isn't on `DueHabitDto` yet — it's added here for the dialog
 * component ahead of the backend wiring that will populate it.
 */
export interface DialogHabit extends DueHabit {
  meta: string | null;
}

/** Which calendar events count as a "real meeting" (design spec §3.6 / §4.5) — mirrors `CalendarMode`. */
export type CalendarMode = "all" | "with-others";

/** The app-wide settings shown in the Settings window (design spec §3.6) — mirrors `Config`. */
export interface Config {
  day_rollover: string;
  day_window_start: string;
  day_window_end: string;
  calendar_pause_enabled: boolean;
  calendar_mode: CalendarMode;
  idle_enabled: boolean;
  dnd_enabled: boolean;
  mic_pause_enabled: boolean;
  start_at_login: boolean;
}

/** The outcome logged for a habit occurrence (design spec §5) — mirrors `EventAction`. */
export type EventAction = "done" | "skipped" | "snoozed" | "expired";

/**
 * A logged event joined with its habit's name and category (design spec
 * §6.1) — mirrors `LoggedEvent`. `at` and `shown_at` are Unix timestamps
 * (seconds); the backend derives them by treating naive local wall-clock
 * time as if it were UTC, so the frontend must read them back the same way
 * (via UTC getters), not via the browser's own local timezone.
 */
export interface LoggedEvent {
  id: number;
  habit_id: number;
  action: EventAction;
  at: number;
  shown_at: number | null;
  habit_name: string;
  category: Category;
}

/** The four-tile day summary (design spec §3.9/§6.1) — mirrors `DaySummary`. */
export interface DaySummary {
  done_count: number;
  skipped_count: number;
  total_moving_secs: number;
  adherence_pct: number;
}

/** The longest sedentary gap (design spec §3.9/§6.1) — mirrors `SedentaryGap`. */
export interface SedentaryGap {
  duration_secs: number;
  start: number;
  end: number;
}

/**
 * A calendar event shown as a context row in the activity list (design spec
 * §3.9) — mirrors `Meeting`. `start`/`end` are Unix timestamps (seconds) in
 * the same naive-local-as-UTC convention as `LoggedEvent.at`, so meetings
 * interleave with movements on one timeline (read back via UTC getters).
 * `attendee_count` is the number of *other* attendees; `is_call` marks an
 * event with at least one, mirroring the with-others meeting-pause rule.
 */
export interface Meeting {
  title: string;
  start: number;
  end: number;
  attendee_count: number;
  is_call: boolean;
}

/** The date-ranged day-log payload the Stats window renders from (design spec §3.9/§6.1) — mirrors `DayLog`. */
export interface DayLog {
  date: string;
  events: LoggedEvent[];
  summary: DaySummary;
  /** `null` when there's no meaningful sit to report: zero movements, or a single movement on a past day. */
  longest_gap: SedentaryGap | null;
  /** The day's calendar events, filtered per the calendar mode; empty when calendar pausing is off. Context only — never affects the summary or longest sit. */
  meetings: Meeting[];
}
