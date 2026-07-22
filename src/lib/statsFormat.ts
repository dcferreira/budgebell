// Pure formatting helpers for the Stats window (design spec §3.9). The
// backend derives `at`/`shown_at` Unix timestamps by treating naive local
// wall-clock time as if it were UTC (see `types.ts`), so every helper here
// that reads an epoch timestamp does so via UTC — never the browser's own
// local timezone — to read back the original wall-clock instant correctly.

/** `date` formatted as `YYYY-MM-DD`, using the local calendar day — this is
 * just "what day is it right now for the user", unrelated to the epoch
 * timestamps returned by `day_log`. */
export function todayDateString(now: Date): string {
  const year = now.getFullYear();
  const month = String(now.getMonth() + 1).padStart(2, "0");
  const day = String(now.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

/** Shifts a `YYYY-MM-DD` date string by `deltaDays`, wrapping months/years
 * correctly. Parsed at UTC midnight — the date string carries no timezone of
 * its own, and UTC keeps the arithmetic free of local-timezone shifts. */
export function shiftDateString(date: string, deltaDays: number): string {
  const parsed = new Date(`${date}T00:00:00Z`);
  parsed.setUTCDate(parsed.getUTCDate() + deltaDays);
  return parsed.toISOString().slice(0, 10);
}

const dayLabelFormatter = new Intl.DateTimeFormat("en-GB", {
  weekday: "long",
  day: "numeric",
  month: "long",
  year: "numeric",
  timeZone: "UTC",
});

/** The date bar's centre label: weekday + date, e.g. "Tuesday 21 July 2026". */
export function formatDayLabel(date: string): string {
  return dayLabelFormatter.format(new Date(`${date}T00:00:00Z`));
}

/** The date bar's relative tag: "Today" / "Yesterday" / "N days ago" (design
 * spec §3.9), computed from the gap between `date` and today. */
export function relativeDayTag(date: string, now: Date): string {
  const daysAgo = Math.round(
    (new Date(`${todayDateString(now)}T00:00:00Z`).getTime() - new Date(`${date}T00:00:00Z`).getTime()) /
      86_400_000,
  );
  if (daysAgo === 0) return "Today";
  if (daysAgo === 1) return "Yesterday";
  return `${daysAgo} days ago`;
}

/** An epoch-seconds instant formatted as a 24-hour clock time, e.g. "12:05" —
 * read via UTC to match the backend's naive-local-as-UTC convention. */
export function formatClockTime(atSecs: number): string {
  const instant = new Date(atSecs * 1_000);
  const hours = String(instant.getUTCHours()).padStart(2, "0");
  const minutes = String(instant.getUTCMinutes()).padStart(2, "0");
  return `${hours}:${minutes}`;
}

/** A `done` row's duration, e.g. "1m 48s" (design spec §3.9). */
export function formatEventDuration(secs: number): string {
  const minutes = Math.floor(secs / 60);
  const seconds = secs % 60;
  return `${minutes}m ${seconds}s`;
}

/** The Moving summary tile's total, e.g. "24 min" or "1h 05m" (design spec §3.9). */
export function formatMovingTime(secs: number): string {
  if (secs < 3_600) {
    return `${Math.round(secs / 60)} min`;
  }
  const hours = Math.floor(secs / 3_600);
  const minutes = String(Math.round((secs % 3_600) / 60)).padStart(2, "0");
  return `${hours}h ${minutes}m`;
}

/** The longest-sit stat's duration, e.g. "2h 31m" or "45m" (design spec §3.9). */
export function formatGapDuration(secs: number): string {
  if (secs < 3_600) {
    return `${Math.round(secs / 60)}m`;
  }
  const hours = Math.floor(secs / 3_600);
  const minutes = Math.round((secs % 3_600) / 60);
  return `${hours}h ${minutes}m`;
}
