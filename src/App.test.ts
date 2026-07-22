import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import App from "./App.svelte";

describe("App", () => {
  it("renders the toast for the currently due habit", () => {
    // GIVEN the app is mounted
    // WHEN it first renders (no interaction)
    render(App);

    // THEN the corner toast shows the demo habit due right now
    expect(screen.getByText("Lunge-and-reach")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Done" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Skip" })).toBeInTheDocument();
  });
});
