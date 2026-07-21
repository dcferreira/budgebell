# habits

A **fully-local** desktop habits app: it nudges you to do things — movement/exercise drills first, but general habits too — shows you *what* to do (including images and video), and logs what you actually did. Cross-platform: macOS and Linux/GNOME.

> **Non-negotiable: local only.** No network calls, no telemetry, no cloud. All data — habit definitions, media, logs — lives on the local machine. An LLM interacts with it via a *bundled local* MCP server, not a hosted service.

## Origin

Grew out of an anti-sedentary intervention (evidence-based movement snacks — hips/hamstrings/glutes — cued through the day). The stop-gap for that is [Stretchly](https://hovancik.net/stretchly/) plus a local calendar-attendee meeting detector (see `docs/` — TODO to import). This project is the native replacement that does it all properly.

## Requirements (from the brief)

- **Mixed habits** — exercise drills *and* other habits in one system.
- **Bundled local MCP server** — so an LLM can add / list / update habits (and ideally read the log). Fully local (stdio or local socket).
- **Media in the nudge** — display images or videos for a habit.
- **Nudge window** — a **Done** button and a **Skip** shortcut; **stays up until manually dismissed** (no auto-timeout).
- **Logging** — record completed (and skipped) habits with timestamps, locally.
- **Top-bar presence on both macOS and GNOME** — small icons in the menu bar (macOS) / top bar (GNOME). Cross-platform is a firm goal; the exact mechanism is under investigation (see Open questions).

## Feature list (v1 draft — for discussion, not final)

1. **Habit model** — name, description/instructions, optional media (image/video), category (e.g. `exercise`, `general`), and a trigger (interval / time-of-day). Exercise drills carry rep/hold guidance.
2. **Nudge window** — surfaces a due habit with its instructions + media; always-on-top; persists until dismissed; **Done** / **Skip** / (optional **Snooze**) actions.
3. **Scheduler** — interval and/or time-based triggers; idle-aware (don't nag when away); *(later)* meeting-aware pause reusing the calendar-attendee detector.
4. **Local store + logging** — habits and a done/skipped event log (likely SQLite); a simple adherence view.
5. **Local MCP server (bundled)** — tools to add/list/update/disable habits and query the log, so an LLM can manage the app. Local transport only.
6. **Tray / top-bar app** — status icon + quick menu (do-now, pause, stats, quit) on macOS and GNOME.
7. **Global shortcuts** — done / skip / snooze (with a documented caveat around Wayland restrictions).
8. **Local-only guarantee** — no outbound network; media stored locally.

## Open questions

- **Tech stack** — under active research (Electron vs Tauri vs Python+Qt vs …), scored on: tray on macOS + GNOME, media-capable persistent popup, global hotkeys (incl. Wayland), ease of bundling an MCP server, packaging. See the pending tech-stack investigation.
- **GNOME top-bar reality** — likely needs the AppIndicator/StatusNotifierItem GNOME extension; to be confirmed.
- **Wayland global hotkeys** — may be restricted; design a fallback.
- **Meeting-aware pause** — v1 or a later increment?
- **Version control** — `jj` (matches the rest of this machine) vs `git`. Not yet initialised.

## Development

Fully local; no network dependencies. The app is a **Tauri v2** (Rust) core with a
**plain Vite + Svelte 5 + TypeScript** SPA frontend (deliberately not SvelteKit — see
`docs/DESIGN.md` §2), styled with **Tailwind v4**.

### Prerequisites

- Rust (via `rustup`) and Xcode Command Line Tools (macOS)
- Node + `pnpm`

### Layout

- `src/` — Svelte 5 SPA (Vite entry at `index.html`)
- `src-tauri/` — Rust core, Tauri config, tray/window, and (later) the bundled MCP server

### Commands

```sh
pnpm install          # install frontend deps (Rust deps fetch on first build)
pnpm tauri dev        # run the desktop app (Vite on :1420 + Rust core)
pnpm build            # build the frontend bundle (-> dist/)
pnpm check            # type-check the frontend (svelte-check)
pnpm test             # run frontend tests (vitest)
cargo test --manifest-path src-tauri/Cargo.toml   # run Rust tests
```

## Status

Infrastructure stood up: the app builds, runs (`pnpm tauri dev`), and is testable on
both sides (vitest + `cargo test`). Feature work (SQLite schema, nudge popup, tray,
MCP server, seed drills) is next — see `docs/DESIGN.md`.
