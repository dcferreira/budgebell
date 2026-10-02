import { fireEvent, render, screen } from "@testing-library/svelte";
import { beforeEach, describe, expect, it, vi } from "vitest";
import Updates from "./Updates.svelte";

const { check, getVersion, relaunch, listen } = vi.hoisted(() => ({
  check: vi.fn(),
  getVersion: vi.fn(),
  relaunch: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-updater", () => ({ check }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion }));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

// A stand-in for the plugin's `Update`: only the fields and method the
// component touches.
function fakeUpdate(version: string) {
  return { version, downloadAndInstall: vi.fn().mockResolvedValue(undefined) };
}

describe("Updates", () => {
  let checkEventHandler: (() => void) | null;

  beforeEach(() => {
    check.mockReset();
    getVersion.mockReset().mockResolvedValue("0.2.0");
    relaunch.mockReset().mockResolvedValue(undefined);
    checkEventHandler = null;
    listen.mockReset().mockImplementation((_event: string, handler: () => void) => {
      checkEventHandler = handler;
      return Promise.resolve(() => {});
    });
  });

  it("shows the running version", async () => {
    // GIVEN the app reports version 0.2.0 and no update exists
    check.mockResolvedValue(null);

    // WHEN the section renders
    render(Updates);

    // THEN the current version is listed
    expect(await screen.findByText("0.2.0")).toBeInTheDocument();
  });

  it("checks GitHub on open and says when the app is up to date", async () => {
    // GIVEN no newer release
    check.mockResolvedValue(null);

    // WHEN the section renders
    render(Updates);

    // THEN it checked once and reports being current, with no install button
    expect(await screen.findByText("You're up to date.")).toBeInTheDocument();
    expect(check).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("button", { name: /Install/ })).not.toBeInTheDocument();
  });

  it("offers to install a newer release", async () => {
    // GIVEN a newer release on GitHub
    check.mockResolvedValue(fakeUpdate("0.3.0"));

    // WHEN the section renders
    render(Updates);

    // THEN the new version and an install button are shown
    expect(await screen.findByText("Version 0.3.0 is available.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Install and restart" })).toBeEnabled();
  });

  it("installs the update and relaunches the app", async () => {
    // GIVEN a newer release has been found
    const update = fakeUpdate("0.3.0");
    check.mockResolvedValue(update);
    render(Updates);
    const install = await screen.findByRole("button", { name: "Install and restart" });

    // WHEN the user clicks install
    await fireEvent.click(install);

    // THEN it downloads + installs, then relaunches into the new version
    await vi.waitFor(() => expect(relaunch).toHaveBeenCalledTimes(1));
    expect(update.downloadAndInstall).toHaveBeenCalledTimes(1);
  });

  it("reports an install failure and does not relaunch", async () => {
    // GIVEN an update whose install fails
    const update = fakeUpdate("0.3.0");
    update.downloadAndInstall.mockRejectedValue(new Error("signature mismatch"));
    check.mockResolvedValue(update);
    render(Updates);

    // WHEN the user clicks install
    await fireEvent.click(await screen.findByRole("button", { name: "Install and restart" }));

    // THEN the error is shown and the app keeps running
    expect(await screen.findByText(/signature mismatch/)).toBeInTheDocument();
    expect(relaunch).not.toHaveBeenCalled();
  });

  it("reports a failed check instead of claiming to be current", async () => {
    // GIVEN the check fails (offline, or a build the updater can't replace)
    check.mockRejectedValue(new Error("network down"));

    // WHEN the section renders
    render(Updates);

    // THEN the error is surfaced
    expect(await screen.findByText(/network down/)).toBeInTheDocument();
    expect(screen.queryByText("You're up to date.")).not.toBeInTheDocument();
  });

  it("re-checks when the user clicks Check now", async () => {
    // GIVEN the first check found nothing, but a release has since appeared
    check.mockResolvedValueOnce(null).mockResolvedValueOnce(fakeUpdate("0.3.0"));
    render(Updates);
    await screen.findByText("You're up to date.");

    // WHEN the user checks again
    await fireEvent.click(screen.getByRole("button", { name: "Check now" }));

    // THEN the new release is offered
    expect(await screen.findByText("Version 0.3.0 is available.")).toBeInTheDocument();
    expect(check).toHaveBeenCalledTimes(2);
  });

  it("re-checks when the tray's 'Check for updates…' fires", async () => {
    // GIVEN the section is open and up to date
    check.mockResolvedValueOnce(null).mockResolvedValueOnce(fakeUpdate("0.3.0"));
    render(Updates);
    await screen.findByText("You're up to date.");
    expect(listen).toHaveBeenCalledWith("check-for-updates", expect.any(Function));

    // WHEN the tray asks for a check
    checkEventHandler?.();

    // THEN it checks again
    expect(await screen.findByText("Version 0.3.0 is available.")).toBeInTheDocument();
  });

  it("ignores a re-check request while an install is running", async () => {
    // GIVEN an install that has started but not finished
    const update = fakeUpdate("0.3.0");
    update.downloadAndInstall.mockReturnValue(new Promise(() => {}));
    check.mockResolvedValue(update);
    render(Updates);
    await fireEvent.click(await screen.findByRole("button", { name: "Install and restart" }));

    // WHEN the tray asks for a check mid-install
    checkEventHandler?.();

    // THEN no second check runs and install can't be started again
    expect(await screen.findByText("Installing 0.3.0…")).toBeInTheDocument();
    expect(check).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "Install and restart" })).toBeDisabled();
  });

  it("keeps the newest check's result when an older one finishes last", async () => {
    // GIVEN a slow first check that will fail, and a fast second that finds 0.3.0
    let failFirst: (error: Error) => void = () => {};
    check
      .mockReturnValueOnce(new Promise((_resolve, reject) => (failFirst = reject)))
      .mockResolvedValueOnce(fakeUpdate("0.3.0"));
    render(Updates);
    await vi.waitFor(() => expect(check).toHaveBeenCalledTimes(1));

    // WHEN the tray re-checks, and only then does the first check fail
    checkEventHandler?.();
    await screen.findByText("Version 0.3.0 is available.");
    failFirst(new Error("stale timeout"));

    // THEN the newer result stands
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.getByText("Version 0.3.0 is available.")).toBeInTheDocument();
    expect(screen.queryByText(/stale timeout/)).not.toBeInTheDocument();
  });

  it("still checks for updates when the version can't be read", async () => {
    // GIVEN getVersion fails
    getVersion.mockRejectedValue(new Error("no version"));
    check.mockResolvedValue(null);

    // WHEN the section renders
    render(Updates);

    // THEN the check still runs
    expect(await screen.findByText("You're up to date.")).toBeInTheDocument();
  });
});
