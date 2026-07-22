import { fireEvent, render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import CustomPauseDialog from "./CustomPauseDialog.svelte";

// A fixed "now" so preset maths are deterministic: Wednesday 2026-07-22 14:30.
const now = () => new Date(2026, 6, 22, 14, 30);

function renderDialog() {
  const onCancel = vi.fn();
  const onPause = vi.fn();
  render(CustomPauseDialog, { onCancel, onPause, now });
  return { onCancel, onPause };
}

describe("CustomPauseDialog", () => {
  it("renders a 'Resume at' datetime-local input, empty by default", () => {
    // GIVEN the custom pause dialog
    // WHEN it renders
    renderDialog();

    // THEN a datetime-local input labelled "Resume at" is shown, unset
    const input = screen.getByLabelText("Resume at");
    expect(input).toHaveAttribute("type", "datetime-local");
    expect(input).toHaveValue("");
  });

  it("sets the resume time one hour from now when the '1 hour' preset is clicked", async () => {
    // GIVEN a rendered dialog
    renderDialog();

    // WHEN the user clicks the "1 hour" preset
    await fireEvent.click(screen.getByRole("button", { name: "1 hour" }));

    // THEN the input is set to exactly one hour after the injected "now"
    expect(screen.getByLabelText("Resume at")).toHaveValue("2026-07-22T15:30");
  });

  it("sets the resume time to tomorrow 9am when the 'Tomorrow 9am' preset is clicked", async () => {
    // GIVEN a rendered dialog
    renderDialog();

    // WHEN the user clicks the "Tomorrow 9am" preset
    await fireEvent.click(screen.getByRole("button", { name: "Tomorrow 9am" }));

    // THEN the input is set to 09:00 on the following calendar day
    expect(screen.getByLabelText("Resume at")).toHaveValue("2026-07-23T09:00");
  });

  it("sets the resume time to next Monday 9am when that preset is clicked", async () => {
    // GIVEN a rendered dialog, with "now" on a Wednesday
    renderDialog();

    // WHEN the user clicks the "Next Monday 9am" preset
    await fireEvent.click(screen.getByRole("button", { name: "Next Monday 9am" }));

    // THEN the input is set to 09:00 on the following Monday
    expect(screen.getByLabelText("Resume at")).toHaveValue("2026-07-27T09:00");
  });

  it("rolls a Monday 'now' over to the Monday a full week later", async () => {
    // GIVEN "now" already falls on a Monday
    const onCancel = vi.fn();
    const onPause = vi.fn();
    render(CustomPauseDialog, { onCancel, onPause, now: () => new Date(2026, 6, 20, 8, 0) });

    // WHEN the user clicks the "Next Monday 9am" preset
    await fireEvent.click(screen.getByRole("button", { name: "Next Monday 9am" }));

    // THEN it skips ahead to the Monday a full week later, not today
    expect(screen.getByLabelText("Resume at")).toHaveValue("2026-07-27T09:00");
  });

  it("disables Pause until a resume time has been set", () => {
    // GIVEN a freshly rendered dialog with no resume time set
    renderDialog();

    // THEN the Pause button is disabled
    expect(screen.getByRole("button", { name: "Pause" })).toBeDisabled();
  });

  it("calls onPause with the chosen resume instant when Pause is clicked", async () => {
    // GIVEN a dialog with a preset applied
    const { onPause } = renderDialog();
    await fireEvent.click(screen.getByRole("button", { name: "1 hour" }));

    // WHEN the user clicks Pause
    await fireEvent.click(screen.getByRole("button", { name: "Pause" }));

    // THEN onPause fires with that instant
    expect(onPause).toHaveBeenCalledOnce();
    expect(onPause.mock.calls[0][0]).toEqual(new Date(2026, 6, 22, 15, 30));
  });

  it("calls onCancel when Cancel is clicked", async () => {
    // GIVEN a rendered dialog
    const { onCancel } = renderDialog();

    // WHEN the user clicks Cancel
    await fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    // THEN onCancel fires
    expect(onCancel).toHaveBeenCalledOnce();
  });

  it("calls onPause with a manually typed resume time", async () => {
    // GIVEN a rendered dialog
    const { onPause } = renderDialog();

    // WHEN the user types a resume time directly into the input and clicks Pause
    await fireEvent.input(screen.getByLabelText("Resume at"), { target: { value: "2026-08-01T10:15" } });
    await fireEvent.click(screen.getByRole("button", { name: "Pause" }));

    // THEN onPause fires with that instant
    expect(onPause).toHaveBeenCalledWith(new Date(2026, 7, 1, 10, 15));
  });
});
