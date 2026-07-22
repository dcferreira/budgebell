import { fireEvent, render, screen } from "@testing-library/svelte";
import { describe, expect, it, vi } from "vitest";
import PausedCard from "./PausedCard.svelte";

describe("PausedCard", () => {
  it("shows the 'Nudges paused' message", () => {
    // GIVEN nudges are paused
    // WHEN the paused card renders
    render(PausedCard, { onResume: vi.fn() });

    // THEN it reads "Nudges paused"
    expect(screen.getByText("Nudges paused")).toBeInTheDocument();
  });

  it("calls onResume when Resume is clicked", async () => {
    // GIVEN a rendered paused card
    const onResume = vi.fn();
    render(PausedCard, { onResume });

    // WHEN the user clicks Resume
    await fireEvent.click(screen.getByRole("button", { name: "Resume" }));

    // THEN onResume fires
    expect(onResume).toHaveBeenCalledOnce();
  });
});
