import { fireEvent, render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import Toast from "./Toast.svelte";
import type { DueHabit } from "./types";

const habit: DueHabit = {
  habit_id: 1,
  name: "Lunge-and-reach",
  instructions: "5 slow reps/leg, reach overhead",
  media_path: null,
  category: "exercise",
};

function renderToast(overrides: Partial<DueHabit> = {}) {
  const onDone = vi.fn();
  const onSkip = vi.fn();
  const onPause = vi.fn();
  const onExpand = vi.fn();
  render(Toast, {
    habit: { ...habit, ...overrides },
    onDone,
    onSkip,
    onPause,
    onExpand,
  });
  return { onDone, onSkip, onPause, onExpand };
}

describe("Toast", () => {
  it("renders the drill's name and sub-line", () => {
    // GIVEN a habit due right now
    // WHEN the toast renders
    renderToast();

    // THEN the drill name and its instructions (the sub-line) are shown
    expect(screen.getByText("Lunge-and-reach")).toBeInTheDocument();
    expect(screen.getByText("5 slow reps/leg, reach overhead")).toBeInTheDocument();
  });

  it("shows a thumbnail image when the habit has media", () => {
    // GIVEN a habit with a media path
    // WHEN the toast renders
    renderToast({ media_path: "/drills/lunge.png" });

    // THEN the figure is rendered as an image pointing at that media
    expect(screen.getByRole("img")).toHaveAttribute("src", "/drills/lunge.png");
  });

  it("shows a placeholder figure when the habit has no media", () => {
    // GIVEN a habit with no media path
    // WHEN the toast renders
    renderToast({ media_path: null });

    // THEN no image is rendered
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
  });

  it("calls onDone with the habit id when Done is clicked", async () => {
    // GIVEN a rendered toast
    const { onDone } = renderToast();

    // WHEN the user clicks Done
    await fireEvent.click(screen.getByRole("button", { name: "Done" }));

    // THEN onDone fires with the habit's id
    expect(onDone).toHaveBeenCalledTimes(1);
    expect(onDone).toHaveBeenCalledWith(1);
  });

  it("calls onSkip with the habit id when Skip is clicked", async () => {
    // GIVEN a rendered toast
    const { onSkip } = renderToast();

    // WHEN the user clicks Skip
    await fireEvent.click(screen.getByRole("button", { name: "Skip" }));

    // THEN onSkip fires with the habit's id
    expect(onSkip).toHaveBeenCalledTimes(1);
    expect(onSkip).toHaveBeenCalledWith(1);
  });

  it("calls onPause when the pause button is clicked", async () => {
    // GIVEN a rendered toast
    const { onPause } = renderToast();

    // WHEN the user clicks the small Pause button
    await fireEvent.click(screen.getByRole("button", { name: "Pause" }));

    // THEN onPause fires
    expect(onPause).toHaveBeenCalledOnce();
  });

  it("calls onExpand when the card body is clicked", async () => {
    // GIVEN a rendered toast
    const { onExpand, onDone, onSkip, onPause } = renderToast();

    // WHEN the user clicks the card body (name + sub-line + figure)
    await fireEvent.click(screen.getByRole("button", { name: "Show details for Lunge-and-reach" }));

    // THEN onExpand fires, and nothing else does
    expect(onExpand).toHaveBeenCalledOnce();
    expect(onDone).not.toHaveBeenCalled();
    expect(onSkip).not.toHaveBeenCalled();
    expect(onPause).not.toHaveBeenCalled();
  });
});
