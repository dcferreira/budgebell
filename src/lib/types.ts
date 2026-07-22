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
