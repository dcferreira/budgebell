import { describe, expect, it } from "vitest";
import { formatClockTime } from "./statsFormat";

describe("formatClockTime", () => {
  it("formats an epoch in the viewer's local time zone, not UTC", () => {
    // GIVEN a true-UTC epoch whose local wall-clock time is 12:05 on a summer day
    const epoch = new Date(2026, 6, 21, 12, 5).getTime() / 1_000;

    // WHEN it is formatted
    // THEN the local HH:MM is shown, whatever the host zone is
    expect(formatClockTime(epoch)).toBe("12:05");
  });

  it("renders a fixed epoch as the right wall-clock time under a pinned zone", () => {
    // GIVEN the process zone is pinned to New York (UTC-4 in July) and the epoch is 2026-07-21T16:05:00Z
    const original = process.env.TZ;
    process.env.TZ = "America/New_York";
    try {
      const epoch = Date.UTC(2026, 6, 21, 16, 5) / 1_000;

      // WHEN formatted
      // THEN it reads 12:05 local
      expect(formatClockTime(epoch)).toBe("12:05");
    } finally {
      if (original === undefined) delete process.env.TZ;
      else process.env.TZ = original;
    }
  });
});
