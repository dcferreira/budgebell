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
  start_at_login: boolean;
}
