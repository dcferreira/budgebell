# habits — design & build spec

Spec for building the `habits` app. Written as a **self-contained handoff**: a build worker should be able to start from this doc (plus `README.md`) without the originating conversation.

> **Non-negotiable: LOCAL ONLY.** No network calls, no telemetry, no cloud, ever. Habit definitions, media, and logs live on the local machine. The LLM integration is a *bundled local* MCP server (stdio / in-process), never a hosted service. Treat any outbound network dependency as a bug.

## 1. What this is

A fully-local desktop app that nudges you to do habits — movement/exercise drills first (its origin), but general habits too — shows you *what* to do (text + image/video), lets you mark **Done** or **Skip**, keeps the nudge up until you dismiss it, and logs what you did. Cross-platform: **macOS-first**, Linux/GNOME (Wayland) aspirational.

It is the native replacement for a stop-gap currently in use (see §8).

## 2. Tech stack (decided)

| Concern | Choice | Rationale |
|---------|--------|-----------|
| App shell | **Tauri v2** (Rust core) | Modern, small bundles, mature tray/window/packaging; official Rust MCP SDK fits in the same process. |
| Frontend | **Svelte 5** (plain SPA, **not** SvelteKit) | Compiler-based, minimal, pleasant DX for a small app. |
| Build/dev | **Vite** + `@sveltejs/vite-plugin-svelte` | Dev server + HMR + bundling; Tauri points `devUrl`/`frontendDist` at it. |
| Language (UI) | **TypeScript** | — |
| Styling | **Tailwind v4** | Fast to style the popup; plain CSS acceptable. |
| MCP server | **`rmcp`** (official Rust MCP SDK), in-process in the Tauri core | Single bundle, no sidecar, local transport only. |
| Storage + log | **SQLite** via a Rust crate (`rusqlite` or `sqlx`) | Local file; habits + event log. |
| Media | HTML `<video>` / `<img>` in the webview, served from a local media folder via Tauri's asset protocol | On macOS (WKWebView) H.264/MP4 plays fine. Bare filesystem paths can't be loaded from the dev/served origin, so media is read through the asset protocol, scoped to the app's media folder only (LOCAL ONLY preserved). See §5. |

**Considered and rejected:** Flutter (great video via libmpv, but Dart and less "cool" for a personal project); Electron (heavy, old-guard, Wayland global-shortcut regression); Tauri+htmx (needs an embedded HTTP server — against Tauri's IPC grain); Leptos/Dioxus (all-Rust UI, very cool, but smaller/less-paved ecosystem — revisit if desired).

## 3. Cross-platform constraints (design around these — not fixable in code)

1. **GNOME top-bar icon needs a user-installed shell extension** (AppIndicator / StatusNotifierItem). GNOME dropped native tray icons; *every* stack needs the extension. Detect the SNI host; if absent, guide the user to install it, and make the popup window itself a fallback entry point. macOS menu bar "just works".
2. **Global hotkeys are unreliable on GNOME/Wayland by design.** So: the **primary interaction is the popup's on-window buttons + in-window key accelerators** (work everywhere). Global hotkeys are a best-effort enhancement (macOS/X11 solid; GNOME-Wayland only 48+ via the GlobalShortcuts portal with user consent). Never make core actions depend on a global hotkey.
3. **Linux video** goes through WebKitGTK and is flaky on Wayland (DMABUF failures, unbundled codecs). **macOS is unaffected.** When Linux becomes real, the escape hatch is an **mpv sidecar/plugin** (`tauri-plugin-libmpv`) — the same libmpv engine other stacks use — at the cost of compositing a native surface over the webview + bundling `libmpv`. Deferred to the Linux phase.
4. **Wayland forbids client-set absolute window position** — precise popup placement is compositor-controlled on Linux. Always-on-top and frameless still work.

## 4. Feature list (v1)

1. **Habit model** — name, description/instructions, optional media (image/video path), category (`exercise` | `general`), trigger (interval and/or time-of-day), enabled flag. Exercise drills carry reps/holds in the instructions.
2. **Nudge window** — always-on-top, frameless; shows instructions + media; **persists until dismissed** (no auto-timeout); **Done** / **Skip** buttons + in-window accelerators; optional Snooze.
   - **Toast layout:** an 88px media thumbnail (image; placeholder figure when a habit has none) beside the title and a two-line instruction clamp; a small circular Pause icon-chip top-right (never overlapping the title). Clicking the card body expands it into the dialog.
   - **Dialog:** full instructions + a media banner (image now; video is extension-classified and deferred), Done/Skip/Snooze, and a **collapse/back control** that returns to the toast. Media rendering is specified in `docs/superpowers/specs/2026-07-22-habits-media-design.md`.
3. **Scheduler** — interval and/or time-of-day triggers; idle-aware (don't nag when away); *(later)* meeting-aware pause (see §7).
4. **Local store + logging** — habits table + a done/skipped event log with timestamps; a simple adherence view.
5. **Bundled local MCP server** — see §6.
6. **Tray / top-bar app** — status icon + menu (do-now, pause, stats, quit); macOS menu bar + GNOME SNI.
7. **Global shortcuts** — done/skip/snooze, best-effort (per §3.2).
8. **Local-only guarantee** — no outbound network; media stored locally.

## 5. Architecture sketch

- **Rust core (Tauri):** owns the SQLite store, the scheduler (a timer that decides when a habit is due and opens the popup window), the tray icon/menu, and the in-process MCP server. Exposes Tauri commands to the Svelte UI (`list_habits`, `complete_habit`, `skip_habit`, `snooze_habit`, `log_query`, …).
- **Svelte UI:** two surfaces — (a) the **nudge popup** (media + instructions + Done/Skip), and (b) a **management view** (list/add/edit habits, see the log/adherence).
- **Media storage:** habit `media_path` is a **relative filename** within a local
  media folder — `<app_data_dir>/media`
  (`~/Library/Application Support/com.dcferreira.habits/media` on macOS), created
  on first run. The webview reads it via Tauri's **asset protocol**, scoped to
  that folder only (`assetProtocol.scope = ["$APPDATA/media/**"]`). The command
  layer resolves the relative name to an absolute path (a pure, path-traversal-safe
  helper) at the IPC boundary; `App.svelte` converts it to an asset URL with
  `convertFileSrc`, so leaf components render a ready-made `mediaUrl`. Nothing
  leaves the machine (LOCAL ONLY). Remote URLs are out of scope; the planned
  convenience is download-once-into-the-folder, then render from disk.
- **SQLite schema (starting point):**
  - `habits(id, name, description, media_path, category, trigger_kind, trigger_config_json, enabled, created_at)`
  - `events(id, habit_id, action ['done'|'skipped'|'snoozed'], at)`
- **MCP tool surface (v1):** `add_habit`, `list_habits`, `list_rotations`, `update_habit`, `disable_habit`, `log_event` (optional), `query_log`, `day_log`. Local transport (stdio or in-process). This is how an LLM manages the app. `list_rotations` lets a caller discover a valid `rotation_id` (and a rotation's current members) before adding a rotation-member habit, which `add_habit` otherwise rejects.

## 6. The MCP server

Bundled in the Rust core via `rmcp`. Lets a local LLM add/list/update/disable habits and read the log — the "an LLM can add stuff to it" requirement. **Local only.** Decide stdio vs in-process during build; either is fine as long as nothing leaves the machine.

## 7. Seed content — evidence-based movement habits (default `exercise` habits)

Ship these as default habits (rotate them; ~one every 30 min is the evidence sweet spot). Kit assumed: floor + standing space, a single step, a light upright bike. Full evidence rationale lives in the vault / the originating worker's Result.

- **Lunge-and-reach** — 5 slow reps/leg, reach overhead *(hips + glutes; RCT-backed anchor — weight it higher in rotation)*
- **Glute bridges** — 20 (or single-leg 10/side)
- **Single-leg Romanian deadlift** — 8/leg, 3s lower *(hamstrings, eccentric)*
- **90/90 hip switches** — 10 slow switches
- **Half-kneeling hip-flexor stretch** — 45s/side
- **Light cardio snack** — 3 min easy bike or step-ups
- **Wall sit 40s → deep squat hold 45s**
- **Side-lying hip abduction** — 15/side *(gluteus medius)*

Plus a 2–3×/week **loaded session** (progress bridges → hip thrusts, add a real RDL) — strengthening needs progressive load, not just mobility breaks.

**Evidence basis (summary):** frequent short movement breaks beat one big workout (Diaz: 5-min light activity every 30 min; Buffey meta-analysis: 2–5 min every 20–30 min). Standing desk is a baseline, not the fix — movement is the active ingredient. For tight hips/hamstrings/inhibited glutes, loaded movement through range beats passive stretching. Adherence: implementation intentions + habit-stacking + timed prompts.

## 8. Relationship to the stop-gap (context, not part of this build)

Until this app exists, movement nudging runs on **Stretchly** (configured with the drills above) + a **local calendar-attendee meeting detector** that toggles Do Not Disturb so Stretchly pauses during real meetings (≥1 other attendee) but not solo focus blocks. Artefacts of that detector — a working EventKit Swift CLI (`~/.local/bin/meeting-now`) + a launchd watcher — are a proven prototype of the **meeting-aware pause** we may fold into this app later (§3/§4.3). See the originating worker's Result.

## 9. First steps for the build worker

1. **Toolchain:** install Rust (`rustup`), Node (Homebrew), and the Tauri CLI. Ensure Xcode Command Line Tools (Swift is already present, so likely OK).
2. **Scaffold:** `create-tauri-app` → Svelte + TypeScript template (gives Tauri v2 + Vite + Svelte + plugin wired up). Confirm `tauri dev` runs.
3. Add **Tailwind v4** to the Vite/Svelte setup.
4. Stand up the **SQLite schema** (§5) and a couple of Tauri commands (`list_habits`, `complete_habit`).
5. Build the **nudge popup window** (always-on-top, frameless, persist-until-dismissed) with a Done/Skip proof-of-concept and a bundled `<video>`.
6. Add the **tray icon** + menu.
7. Wire the **`rmcp` MCP server** in-process with `add_habit` / `list_habits`.
8. Seed the default movement habits (§7).

Work in a jj workspace; small, reviewable commits.

## 10. Open questions

- Trigger model detail (fixed interval vs per-habit schedules vs both) — settle in build.
- MCP transport: stdio vs in-process — settle in build.
- Whether meeting-aware pause (§8) is a v1 feature or a later increment.

**Settled during build (kept for the record):**
- Media: read from a local scoped folder via the asset protocol; relative
  `media_path`; image first, video deferred; remote URLs out of scope (§5,
  `docs/superpowers/specs/2026-07-22-habits-media-design.md`).
