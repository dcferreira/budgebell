import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import Settings from "./Settings.svelte";
import type { Config } from "./types";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

// The default config from the design spec §3.6/§7: calendar pause on,
// "events with someone else", rollover 04:00, day window 09:00–18:00.
function sampleConfig(): Config {
  return {
    day_rollover: "04:00",
    day_window_start: "09:00",
    day_window_end: "18:00",
    calendar_pause_enabled: true,
    calendar_mode: "with-others",
    idle_enabled: true,
    dnd_enabled: true,
    start_at_login: false,
  };
}

async function renderSettings(config: Config = sampleConfig()) {
  invoke.mockImplementation((command: string) => {
    if (command === "get_config") return Promise.resolve(config);
    if (command === "set_config") return Promise.resolve(undefined);
    throw new Error(`unexpected command: ${command}`);
  });
  render(Settings);
  // Wait for the async get_config load to settle before returning.
  await screen.findByLabelText("Pause using my calendar");
}

describe("Settings", () => {
  beforeEach(() => {
    invoke.mockReset();
  });

  it("loads the config from get_config and reflects every field's value", async () => {
    // GIVEN a backend config with non-default values
    const config: Config = {
      day_rollover: "05:30",
      day_window_start: "08:00",
      day_window_end: "17:00",
      calendar_pause_enabled: false,
      calendar_mode: "all",
      idle_enabled: false,
      dnd_enabled: false,
      start_at_login: true,
    };

    // WHEN the Settings window renders
    await renderSettings(config);

    // THEN every control reflects exactly what get_config returned
    expect(invoke).toHaveBeenCalledWith("get_config");
    expect(screen.getByLabelText("Pause using my calendar")).not.toBeChecked();
    expect(screen.getByRole("button", { name: "All calendar events" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "Events with someone else" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
    expect(screen.getByLabelText("Don't nudge when idle")).not.toBeChecked();
    expect(screen.getByLabelText("Respect Do Not Disturb / Focus")).not.toBeChecked();
    expect(screen.getByLabelText("Day rollover time")).toHaveValue("05:30");
    expect(screen.getByLabelText("Day window start")).toHaveValue("08:00");
    expect(screen.getByLabelText("Day window end")).toHaveValue("17:00");
    expect(screen.getByLabelText("Start at login")).toBeChecked();
  });

  it("defaults the calendar mode sub-choice to 'events with someone else'", async () => {
    // GIVEN the design spec's default config
    // WHEN the Settings window renders
    await renderSettings();

    // THEN "Events with someone else" is the selected sub-choice
    expect(screen.getByRole("button", { name: "Events with someone else" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(screen.getByRole("button", { name: "All calendar events" })).toHaveAttribute(
      "aria-pressed",
      "false",
    );
  });

  it("enables the calendar mode sub-choice while 'Pause using my calendar' is on", async () => {
    // GIVEN the default config, where calendar pause is enabled
    await renderSettings();

    // THEN both sub-choice buttons are enabled
    expect(screen.getByRole("button", { name: "Events with someone else" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "All calendar events" })).toBeEnabled();
  });

  it("disables (dims) the calendar mode sub-choice when 'Pause using my calendar' is off", async () => {
    // GIVEN a config with calendar pause disabled
    await renderSettings({ ...sampleConfig(), calendar_pause_enabled: false });

    // THEN both sub-choice buttons are disabled
    expect(screen.getByRole("button", { name: "Events with someone else" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "All calendar events" })).toBeDisabled();
  });

  it("re-enables the sub-choice as soon as the toggle is switched back on", async () => {
    // GIVEN calendar pause starts off
    await renderSettings({ ...sampleConfig(), calendar_pause_enabled: false });
    expect(screen.getByRole("button", { name: "All calendar events" })).toBeDisabled();

    // WHEN the user switches "Pause using my calendar" back on
    await fireEvent.click(screen.getByLabelText("Pause using my calendar"));

    // THEN the sub-choice becomes enabled again
    expect(await screen.findByRole("button", { name: "All calendar events" })).toBeEnabled();
  });

  it("persists the toggle via set_config, keeping every other field unchanged", async () => {
    // GIVEN the default config, rendered
    await renderSettings();

    // WHEN the user turns off "Pause using my calendar"
    await fireEvent.click(screen.getByLabelText("Pause using my calendar"));

    // THEN set_config is called with only that field flipped
    expect(invoke).toHaveBeenCalledWith("set_config", {
      config: { ...sampleConfig(), calendar_pause_enabled: false },
    });
  });

  it("persists the chosen calendar mode when a sub-choice is clicked", async () => {
    // GIVEN the default config, rendered
    await renderSettings();

    // WHEN the user picks "All calendar events"
    await fireEvent.click(screen.getByRole("button", { name: "All calendar events" }));

    // THEN set_config is called with the new mode
    expect(invoke).toHaveBeenCalledWith("set_config", {
      config: { ...sampleConfig(), calendar_mode: "all" },
    });
  });

  it("persists 'Don't nudge when idle' round trip", async () => {
    // GIVEN the default config, rendered
    await renderSettings();

    // WHEN idle-nudging is turned off
    await fireEvent.click(screen.getByLabelText("Don't nudge when idle"));

    // THEN set_config is called with that field flipped
    expect(invoke).toHaveBeenCalledWith("set_config", {
      config: { ...sampleConfig(), idle_enabled: false },
    });
  });

  it("persists 'Respect Do Not Disturb / Focus' round trip", async () => {
    // GIVEN the default config, rendered
    await renderSettings();

    // WHEN DND-respecting is turned off
    await fireEvent.click(screen.getByLabelText("Respect Do Not Disturb / Focus"));

    // THEN set_config is called with that field flipped
    expect(invoke).toHaveBeenCalledWith("set_config", {
      config: { ...sampleConfig(), dnd_enabled: false },
    });
  });

  it("persists 'Start at login' round trip", async () => {
    // GIVEN the default config, rendered
    await renderSettings();

    // WHEN "Start at login" is turned on
    await fireEvent.click(screen.getByLabelText("Start at login"));

    // THEN set_config is called with that field flipped
    expect(invoke).toHaveBeenCalledWith("set_config", {
      config: { ...sampleConfig(), start_at_login: true },
    });
  });

  it("persists a changed day rollover time", async () => {
    // GIVEN the default config, rendered
    await renderSettings();

    // WHEN the user changes the day rollover time
    await fireEvent.input(screen.getByLabelText("Day rollover time"), { target: { value: "03:00" } });

    // THEN set_config is called with the new rollover
    expect(invoke).toHaveBeenCalledWith("set_config", {
      config: { ...sampleConfig(), day_rollover: "03:00" },
    });
  });

  it("persists a changed global day window start and end", async () => {
    // GIVEN the default config, rendered
    await renderSettings();

    // WHEN the user changes the window start and end
    await fireEvent.input(screen.getByLabelText("Day window start"), { target: { value: "08:30" } });
    await fireEvent.input(screen.getByLabelText("Day window end"), { target: { value: "19:00" } });

    // THEN set_config is called with each new value
    expect(invoke).toHaveBeenCalledWith("set_config", {
      config: { ...sampleConfig(), day_window_start: "08:30" },
    });
    expect(invoke).toHaveBeenCalledWith("set_config", {
      config: { ...sampleConfig(), day_window_start: "08:30", day_window_end: "19:00" },
    });
  });
});
