import { render, screen, waitFor } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import StatsWindow from "./StatsWindow.svelte";
import type { DayLog } from "./types";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

// A fixed "now" for deterministic date-bar behaviour: Tuesday 2026-07-21,
// 14:00 local. Constructed from local components so the injected clock is
// independent of the test runner's own timezone.
const today = () => new Date(2026, 6, 21, 14, 0, 0);

function emptyDayLog(date: string): DayLog {
  return {
    date,
    events: [],
    summary: { done_count: 0, skipped_count: 0, total_moving_secs: 0, adherence_pct: 0 },
    longest_gap: null,
  };
}

function sampleDayLog(): DayLog {
  return {
    date: "2026-07-21",
    events: [
      {
        id: 1,
        habit_id: 1,
        action: "done",
        at: 12 * 3_600 + 5 * 60, // 12:05
        shown_at: 12 * 3_600 + 5 * 60 - 108,
        habit_name: "Lunge-and-reach",
        category: "exercise",
      },
      {
        id: 2,
        habit_id: 2,
        action: "skipped",
        at: 14 * 3_600 + 35 * 60, // 14:35
        shown_at: 14 * 3_600 + 35 * 60 - 20,
        habit_name: "Glute bridges",
        category: "exercise",
      },
    ],
    summary: { done_count: 1, skipped_count: 1, total_moving_secs: 108, adherence_pct: 50 },
    longest_gap: {
      duration_secs: 2 * 3_600 + 31 * 60,
      start: 12 * 3_600 + 5 * 60,
      end: 14 * 3_600 + 35 * 60,
    },
  };
}

async function renderStats(dayLogByDate: Record<string, DayLog> = { "2026-07-21": sampleDayLog() }) {
  invoke.mockImplementation((command: string, args?: Record<string, unknown>) => {
    if (command === "day_log") {
      const date = args?.date as string;
      return Promise.resolve(dayLogByDate[date] ?? emptyDayLog(date));
    }
    throw new Error(`unexpected command: ${command}`);
  });
  const view = render(StatsWindow, { now: today });
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("day_log", { date: "2026-07-21" }));
  return view;
}

describe("StatsWindow", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  describe("date bar", () => {
    it("defaults to today, showing the weekday, date, and the 'Today' relative tag", async () => {
      // GIVEN the app's injected clock is Tuesday 2026-07-21
      // WHEN the Stats window renders
      await renderStats();

      // THEN it defaults to today and labels it accordingly
      expect(screen.getByText(/Tuesday/)).toBeInTheDocument();
      expect(screen.getByText(/21 July 2026/)).toBeInTheDocument();
      expect(screen.getByText("Today")).toBeInTheDocument();
    });

    it("disables the forward arrow while viewing today", async () => {
      // GIVEN the Stats window defaults to today
      await renderStats();

      // THEN the next-day arrow is disabled — there are no future days
      expect(screen.getByRole("button", { name: "Next day" })).toBeDisabled();
    });

    it("moves back a day and re-enables forward when the prev arrow is clicked", async () => {
      // GIVEN the Stats window is showing today
      const { getByRole } = await renderStats();

      // WHEN the user clicks the previous-day arrow
      await getByRole("button", { name: "Previous day" }).click();

      // THEN it fetches and shows yesterday, tagged "Yesterday", and forward re-enables
      await waitFor(() => expect(invoke).toHaveBeenCalledWith("day_log", { date: "2026-07-20" }));
      expect(await screen.findByText("Yesterday")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "Next day" })).toBeEnabled();
    });

    it("tags a day further back as 'N days ago'", async () => {
      // GIVEN the Stats window is showing today
      const { getByRole } = await renderStats();

      // WHEN the user steps back three days
      await getByRole("button", { name: "Previous day" }).click();
      await waitFor(() => expect(invoke).toHaveBeenCalledWith("day_log", { date: "2026-07-20" }));
      await getByRole("button", { name: "Previous day" }).click();
      await waitFor(() => expect(invoke).toHaveBeenCalledWith("day_log", { date: "2026-07-19" }));
      await getByRole("button", { name: "Previous day" }).click();
      await waitFor(() => expect(invoke).toHaveBeenCalledWith("day_log", { date: "2026-07-18" }));

      // THEN the relative tag reads "3 days ago"
      expect(await screen.findByText("3 days ago")).toBeInTheDocument();
    });

    it("does not fetch a future day when the (disabled) next arrow is clicked while on today", async () => {
      // GIVEN the Stats window is showing today
      const { getByRole } = await renderStats();
      invoke.mockClear();

      // WHEN the disabled next-day arrow is clicked anyway
      await getByRole("button", { name: "Next day" }).click();

      // THEN no further fetch happens — the date bar never goes past today
      expect(invoke).not.toHaveBeenCalled();
    });
  });

  describe("summary strip", () => {
    it("renders the Done, Skipped, Moving, and Adherence tiles from the day summary", async () => {
      // GIVEN a day with one done (108s) and one skipped event
      await renderStats();

      // THEN the four summary tiles reflect it
      expect(screen.getByTestId("summary-done")).toHaveTextContent("1");
      expect(screen.getByTestId("summary-skipped")).toHaveTextContent("1");
      expect(screen.getByTestId("summary-moving")).toHaveTextContent("2 min");
      expect(screen.getByTestId("summary-adherence")).toHaveTextContent("50%");
    });

    it("shows 0% adherence when nothing was done or skipped", async () => {
      // GIVEN an empty day
      await renderStats({ "2026-07-21": emptyDayLog("2026-07-21") });

      // THEN adherence reads 0%, not NaN or undefined
      expect(screen.getByTestId("summary-adherence")).toHaveTextContent("0%");
    });
  });

  describe("longest sit", () => {
    it("renders the longest gap's duration and time window", async () => {
      // GIVEN a longest gap of 2h31m spanning 12:05-14:35
      await renderStats();

      // THEN the longest-sit stat shows both the duration and the window
      const longestSit = screen.getByTestId("longest-sit");
      expect(longestSit).toHaveTextContent("2h 31m");
      expect(longestSit).toHaveTextContent("12:05");
      expect(longestSit).toHaveTextContent("14:35");
    });

    it("shows a placeholder instead of a fabricated gap when there's no meaningful sit", async () => {
      // GIVEN a day with no movements, so the backend reports no gap
      await renderStats({ "2026-07-21": emptyDayLog("2026-07-21") });

      // THEN the longest-sit stat explains there's nothing to report, rather
      // than inventing a gap from the day window
      const longestSit = screen.getByTestId("longest-sit");
      expect(longestSit).toHaveTextContent("Not enough movements yet");
    });
  });

  describe("activity list", () => {
    it("shows the header counting drills, done, and skipped", async () => {
      // GIVEN one done and one skipped event
      await renderStats();

      // THEN the header summarises the merged list
      expect(screen.getByText("2 drills · 1 done · 1 skipped")).toBeInTheDocument();
    });

    it("renders a done row with a filled dot, its time, name, category, and green duration", async () => {
      // GIVEN a done event at 12:05 lasting 108s (1m 48s)
      await renderStats();

      // THEN the done row shows every element
      const doneRow = screen.getByTestId("activity-row-1");
      expect(doneRow).toHaveTextContent("12:05");
      expect(doneRow).toHaveTextContent("Lunge-and-reach");
      expect(doneRow).toHaveTextContent("exercise");
      expect(doneRow).toHaveTextContent("1m 48s");
      expect(doneRow.querySelector('[data-testid="status-dot-done"]')).toBeInTheDocument();
    });

    it("renders a skipped row with a hollow ring, its time, struck/dimmed name, and a SKIPPED label", async () => {
      // GIVEN a skipped event at 14:35
      await renderStats();

      // THEN the skipped row shows the hollow ring, dimmed/struck name, and label
      const skippedRow = screen.getByTestId("activity-row-2");
      expect(skippedRow).toHaveTextContent("14:35");
      expect(skippedRow).toHaveTextContent("Glute bridges");
      expect(skippedRow).toHaveTextContent("SKIPPED");
      expect(skippedRow.querySelector('[data-testid="status-ring-skipped"]')).toBeInTheDocument();

      const name = screen.getByText("Glute bridges");
      expect(name).toHaveClass("line-through");
    });

    it("merges done and skipped events in chronological order", async () => {
      // GIVEN a done event at 12:05 followed by a skipped event at 14:35
      await renderStats();

      // THEN the rows render in that chronological order
      const rows = screen.getAllByTestId(/^activity-row-/);
      expect(rows.map((row) => row.dataset.testid)).toEqual(["activity-row-1", "activity-row-2"]);
    });
  });

  describe("idle footnote", () => {
    it("explains that drills the user was away for are withdrawn and never logged", async () => {
      // GIVEN any day
      await renderStats();

      // THEN the footnote makes the idle-withdraw rule explicit
      expect(screen.getByText(/withdrawn and never logged/i)).toBeInTheDocument();
    });
  });
});
