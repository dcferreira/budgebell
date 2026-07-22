import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App.svelte";
import type { DueHabit } from "./lib/types";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

const dueHabit: DueHabit = {
  habit_id: 7,
  name: "Glute bridges",
  instructions: "20, or single-leg 10/side",
  media_path: null,
  category: "exercise",
};

// Renders the app as the real toast window (`?view=toast`) with `current_due`
// resolving to `habit`, so the invoke-wired commands (design spec §10
// "commands") can be exercised end-to-end.
async function renderToastView(habit: DueHabit | null = dueHabit) {
  window.history.pushState({}, "", "/?view=toast");
  invoke.mockImplementation((command: string) => {
    if (command === "current_due") return Promise.resolve(habit);
    return Promise.resolve(undefined);
  });
  listen.mockResolvedValue(() => {});
  const view = render(App);
  if (habit) {
    await screen.findByText(habit.name);
  }
  return view;
}

describe("App", () => {
  beforeEach(() => {
    invoke.mockReset();
    listen.mockReset();
    window.history.pushState({}, "", "/");
  });

  it("renders the toast for the currently due habit", () => {
    // GIVEN the app is mounted
    // WHEN it first renders (no interaction)
    render(App);

    // THEN the corner toast shows the demo habit due right now
    expect(screen.getByText("Lunge-and-reach")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Done" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Skip" })).toBeInTheDocument();
  });

  it("expands to the full dialog when the toast body is clicked, replacing the toast", async () => {
    // GIVEN the toast is showing
    render(App);

    // WHEN the user clicks the toast body
    await fireEvent.click(screen.getByRole("button", { name: "Show details for Lunge-and-reach" }));

    // THEN the expanded dialog renders the habit's full details in place of the toast
    expect(screen.getByRole("heading", { name: "Lunge-and-reach" })).toBeInTheDocument();
    expect(screen.getByText("Exercise")).toBeInTheDocument();
    expect(screen.getByText("5 slow reps/leg, reach overhead")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Snooze" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Show details for Lunge-and-reach" })).not.toBeInTheDocument();
  });

  it("invokes complete_habit and closes the dialog when Done is clicked in the expanded dialog", async () => {
    // GIVEN the dialog is showing for the currently due habit
    await renderToastView();
    await fireEvent.click(screen.getByRole("button", { name: "Show details for Glute bridges" }));

    // WHEN the user clicks Done in the dialog
    await fireEvent.click(screen.getByRole("button", { name: "Done" }));

    // THEN complete_habit is invoked with the habit's id, and both the dialog and toast close
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("complete_habit", { habitId: 7 }));
    expect(screen.queryByRole("heading", { name: "Glute bridges" })).not.toBeInTheDocument();
    expect(screen.queryByText("Glute bridges")).not.toBeInTheDocument();
  });

  it("invokes skip_habit and closes the dialog when Skip is clicked in the expanded dialog", async () => {
    // GIVEN the dialog is showing for the currently due habit
    await renderToastView();
    await fireEvent.click(screen.getByRole("button", { name: "Show details for Glute bridges" }));

    // WHEN the user clicks Skip in the dialog
    await fireEvent.click(screen.getByRole("button", { name: "Skip" }));

    // THEN skip_habit is invoked with the habit's id, and both the dialog and toast close
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("skip_habit", { habitId: 7 }));
    expect(screen.queryByText("Glute bridges")).not.toBeInTheDocument();
  });

  it("invokes snooze_habit and closes the dialog when Snooze is clicked in the expanded dialog", async () => {
    // GIVEN the dialog is showing for the currently due habit
    await renderToastView();
    await fireEvent.click(screen.getByRole("button", { name: "Show details for Glute bridges" }));

    // WHEN the user clicks Snooze in the dialog
    await fireEvent.click(screen.getByRole("button", { name: "Snooze" }));

    // THEN snooze_habit is invoked with the habit's id, and both the dialog and toast close
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("snooze_habit", { habitId: 7 }));
    expect(screen.queryByText("Glute bridges")).not.toBeInTheDocument();
  });

  it("invokes pause and shows the paused state when 'Turn off nudges' is clicked in the dialog", async () => {
    // GIVEN the dialog is showing for the currently due habit — the design
    // spec (§3.7) treats the dialog's "Turn off nudges" as the same
    // off-switch as the toast's own Pause button
    await renderToastView();
    await fireEvent.click(screen.getByRole("button", { name: "Show details for Glute bridges" }));

    // WHEN the user clicks "Turn off nudges"
    await fireEvent.click(screen.getByRole("button", { name: "Turn off nudges" }));

    // THEN pause is invoked, and the paused card replaces the dialog
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("pause", { durationSecs: 30 * 60 }));
    await waitFor(() => expect(screen.getByText("Nudges paused")).toBeInTheDocument());
    expect(screen.queryByText("Glute bridges")).not.toBeInTheDocument();
  });

  it("invokes resume and clears the paused state when Resume is clicked in the paused card", async () => {
    // GIVEN the paused card is showing (nudges turned off via the dialog)
    await renderToastView();
    await fireEvent.click(screen.getByRole("button", { name: "Show details for Glute bridges" }));
    await fireEvent.click(screen.getByRole("button", { name: "Turn off nudges" }));
    await waitFor(() => expect(screen.getByText("Nudges paused")).toBeInTheDocument());

    // WHEN the user clicks Resume
    await fireEvent.click(screen.getByRole("button", { name: "Resume" }));

    // THEN resume is invoked, and the paused card is no longer shown
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("resume", undefined));
    expect(screen.queryByText("Nudges paused")).not.toBeInTheDocument();
  });

  it("collapses back to the toast when the dialog's Settings link is clicked", async () => {
    // GIVEN the dialog is showing for the currently due habit
    await renderToastView();
    await fireEvent.click(screen.getByRole("button", { name: "Show details for Glute bridges" }));

    // WHEN the user clicks the footer Settings link
    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    // THEN the dialog collapses back to the toast, with nothing invoked for it
    expect(screen.getByRole("button", { name: "Show details for Glute bridges" })).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("complete_habit", expect.anything());
    expect(invoke).not.toHaveBeenCalledWith("skip_habit", expect.anything());
    expect(invoke).not.toHaveBeenCalledWith("snooze_habit", expect.anything());
    expect(invoke).not.toHaveBeenCalledWith("pause", expect.anything());
  });
});
