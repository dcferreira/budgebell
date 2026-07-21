import { render, screen } from "@testing-library/svelte";
import { describe, expect, it } from "vitest";
import App from "./App.svelte";

describe("App", () => {
  it("renders the app title and greet control", () => {
    // GIVEN the app is mounted
    // WHEN it first renders (no interaction)
    render(App);

    // THEN the title and the greet button are present, proving the Svelte +
    // Testing Library harness is correctly wired.
    expect(screen.getByRole("heading", { name: "habits" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Greet" })).toBeInTheDocument();
  });
});
