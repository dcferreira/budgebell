import { fireEvent, render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import Dialog from "./Dialog.svelte";
import type { DialogHabit } from "./types";

const habit: DialogHabit = {
  habit_id: 1,
  name: "Lunge-and-reach",
  instructions: "Step forward into a slow lunge, reaching both arms overhead, then return and switch legs.",
  media_path: null,
  category: "exercise",
  meta: "5 slow reps/leg",
};

function renderDialog(overrides: Partial<DialogHabit> = {}) {
  const onDone = vi.fn();
  const onSkip = vi.fn();
  const onSnooze = vi.fn();
  const onSettings = vi.fn();
  const onTurnOffNudges = vi.fn();
  const view = render(Dialog, {
    habit: { ...habit, ...overrides },
    onDone,
    onSkip,
    onSnooze,
    onSettings,
    onTurnOffNudges,
  });
  return { ...view, onDone, onSkip, onSnooze, onSettings, onTurnOffNudges };
}

describe("Dialog", () => {
  it("renders the title, category pill, full instructions, and meta line", () => {
    // GIVEN a habit due right now, with a meta line
    // WHEN the dialog renders
    renderDialog();

    // THEN the title, category pill, full instructions, and meta line all show
    expect(screen.getByRole("heading", { name: "Lunge-and-reach" })).toBeInTheDocument();
    expect(screen.getByText("Exercise")).toBeInTheDocument();
    expect(
      screen.getByText(
        "Step forward into a slow lunge, reaching both arms overhead, then return and switch legs.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("5 slow reps/leg")).toBeInTheDocument();
  });

  it("omits the meta line when the habit has none", () => {
    // GIVEN a habit with no meta line
    // WHEN the dialog renders
    const { container } = renderDialog({ meta: null });

    // THEN no meta line element is rendered
    expect(container.querySelector("[data-testid='meta-line']")).not.toBeInTheDocument();
  });

  it("shows a general category pill for a general habit", () => {
    // GIVEN a general (non-exercise) habit
    // WHEN the dialog renders
    renderDialog({ category: "general" });

    // THEN the pill reads "General"
    expect(screen.getByText("General")).toBeInTheDocument();
  });

  it("shows an image in the media area when the media path is an image", () => {
    // GIVEN a habit with an image media path
    // WHEN the dialog renders
    renderDialog({ media_path: "/drills/lunge.png" });

    // THEN the media area renders an image pointing at that path
    expect(screen.getByRole("img", { name: "Lunge-and-reach" })).toHaveAttribute(
      "src",
      "/drills/lunge.png",
    );
  });

  it("shows a video in the media area when the media path is a video", () => {
    // GIVEN a habit with a video media path
    // WHEN the dialog renders
    const { container } = renderDialog({ media_path: "/drills/lunge.mp4" });

    // THEN the media area renders a video pointing at that path
    const video = container.querySelector("video");
    expect(video).not.toBeNull();
    expect(video).toHaveAttribute("src", "/drills/lunge.mp4");
  });

  it("shows a placeholder in the media area when the habit has no media", () => {
    // GIVEN a habit with no media path
    // WHEN the dialog renders
    const { container } = renderDialog({ media_path: null });

    // THEN neither an image nor a video is rendered
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(container.querySelector("video")).toBeNull();
  });

  it("calls onDone with the habit id when Done is clicked", async () => {
    // GIVEN a rendered dialog
    const { onDone } = renderDialog();

    // WHEN the user clicks Done
    await fireEvent.click(screen.getByRole("button", { name: "Done" }));

    // THEN onDone fires with the habit's id
    expect(onDone).toHaveBeenCalledTimes(1);
    expect(onDone).toHaveBeenCalledWith(1);
  });

  it("calls onSkip with the habit id when Skip is clicked", async () => {
    // GIVEN a rendered dialog
    const { onSkip } = renderDialog();

    // WHEN the user clicks Skip
    await fireEvent.click(screen.getByRole("button", { name: "Skip" }));

    // THEN onSkip fires with the habit's id
    expect(onSkip).toHaveBeenCalledTimes(1);
    expect(onSkip).toHaveBeenCalledWith(1);
  });

  it("calls onSnooze with the habit id when Snooze is clicked", async () => {
    // GIVEN a rendered dialog
    const { onSnooze } = renderDialog();

    // WHEN the user clicks Snooze
    await fireEvent.click(screen.getByRole("button", { name: "Snooze" }));

    // THEN onSnooze fires with the habit's id
    expect(onSnooze).toHaveBeenCalledTimes(1);
    expect(onSnooze).toHaveBeenCalledWith(1);
  });

  it("calls onSettings when the footer Settings link is clicked", async () => {
    // GIVEN a rendered dialog
    const { onSettings } = renderDialog();

    // WHEN the user clicks the footer Settings link
    await fireEvent.click(screen.getByRole("button", { name: "Settings" }));

    // THEN onSettings fires
    expect(onSettings).toHaveBeenCalledOnce();
  });

  it("calls onTurnOffNudges when the footer danger link is clicked", async () => {
    // GIVEN a rendered dialog
    const { onTurnOffNudges } = renderDialog();

    // WHEN the user clicks "Turn off nudges"
    await fireEvent.click(screen.getByRole("button", { name: "Turn off nudges" }));

    // THEN onTurnOffNudges fires
    expect(onTurnOffNudges).toHaveBeenCalledOnce();
  });

  it("styles the 'Turn off nudges' link as a danger action", () => {
    // GIVEN a rendered dialog
    renderDialog();

    // THEN the "Turn off nudges" link is styled to signal a destructive action
    expect(screen.getByRole("button", { name: "Turn off nudges" }).className).toMatch(/text-red-/);
  });
});
