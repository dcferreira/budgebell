# habits — Finalised Design Specification

**Status:** Locked. Source of truth for the build.
**Date:** 2026-07-22
**Scope:** A fully-local macOS desktop app that nudges movement/exercise and general habits.

This document is self-contained: a build team can implement from it without any prior conversation. It expands the locked design into implementable sections. Do not invent features beyond what is written here, and do not re-open the decisions recorded below.

---

## 1. Overview & Local-Only Guarantee

`habits` is a macOS desktop application that periodically nudges the user to perform short movement/exercise drills and general habits. Each nudge shows what to do (text plus an optional image or video), lets the user mark it **Done** or **Skip**, and logs the outcome. Nudges are surfaced through a single UI surface — a corner toast — that can expand into a richer dialog.

### Local-only guarantee (NON-NEGOTIABLE)

The app is **fully local**. There is **no network, no telemetry, no cloud — ever**.

- Treat any outbound network dependency as a **bug**, not a feature.
- No analytics, crash reporting, remote config, update pings, or font/asset CDNs.
- All data (habits, rotations, events, settings, media) lives on the user's machine.
- The MCP server (see §7) uses a **local transport only** and exists so a locally-running LLM can manage the app. Nothing it does leaves the machine.
- The calendar integration (see §9) reads the **local** Calendar store via EventKit. It performs no network I/O.

This guarantee shapes every technical decision: prefer bundled/offline capabilities; reject any dependency that phones home.

---

## 2. Tech Stack (decided; infra already built + committed)

The scaffold already exists and is committed. **Build on top of it — do not re-scaffold.**

| Layer | Choice |
| --- | --- |
| Shell / core | **Tauri v2** (Rust core) |
| Frontend | **Plain Svelte 5 SPA** (Vite + TypeScript) — **NOT SvelteKit** |
| Styling | **Tailwind v4** via `@tailwindcss/vite` |
| Storage | **SQLite** via **rusqlite** |
| Local LLM control | **rmcp** (official Rust MCP SDK), in-process |
| Frontend tests | **vitest** + **@testing-library/svelte** (jsdom) |
| Rust tests | **cargo test** |

### What already exists in the scaffold

- `src-tauri/` — Rust core. `lib.rs` has a `greet` smoke command plus a test.
- `src/` — Svelte frontend. `App.svelte` has a `greet` smoke UI.
- `package.json` scripts: `dev`, `build`, `check`, `test`, `tauri`.
- A first commit.

New Rust code must be idiomatic and split into small modules. Svelte 5 code uses **runes** (`$state` / `$derived`) and is fully typed. Styling uses Tailwind v4 utility classes. Match the scaffold's existing style throughout.

---

## 3. Nudge UX

Derived from approved prototypes. **One surface only: the corner toast.** No other nudge style (no popover, ambient, or focus window). **No keyboard shortcuts anywhere** — the app is entirely mouse-driven, and no key hints appear in any UI.

### 3.1 Toast

The primary nudge surface.

- Compact card, pinned to the **top-right corner**.
- **Always-on-top**, **frameless**.
- **Never steals keyboard focus.**
- **Persists until acted on** — no auto-timeout.
- Content: a small figure/thumbnail, the drill name, and a short sub-line.
- Buttons (mouse): **Done**, **Skip**.
- A small **Pause** button at the top-right of the toast.
- Clicking the **card body** expands the toast into the dialog (§3.2).

### 3.2 Expanded dialog

Reached by clicking the toast body.

- **Media**: video or image.
- **Category pill** (e.g. "exercise" / "general").
- **Title**.
- **Full instructions**.
- A **meta line** (reps / duration).
- Actions: **Done**, **Skip**, **Snooze**.
- Footer: a **Settings** (gear) link and a **"Turn off nudges"** (danger) link.
- No key hints anywhere.

### 3.3 Tray (macOS menu-bar) menu

Menu items, in order:

1. **Do a drill now**
2. **Pause nudges** → submenu: **30 min** / **1 hour** / **Custom…**
3. **Today's stats**
4. **Settings…**
5. **Quit**

### 3.4 Custom pause dialog

Opened from **Pause nudges → Custom…**

- A **"Resume at"** control: an HTML `<input type="datetime-local">`, which renders the native WKWebView picker.
- Quick presets: **1 hour**, **Tomorrow 9am**, **Next Monday 9am**.
- Actions: **Cancel**, **Pause**.

### 3.5 Paused state

- **Dimmed tray icon**.
- A small card reading **"Nudges paused"** with a **Resume** button.

### 3.6 Settings window (a real window)

Two groups.

**Quiet rules**

- **"Pause using my calendar"** (essential) with a sub-choice segmented control:
  - **"Events with someone else"** (**DEFAULT**)
  - **"All calendar events"**
- **"Don't nudge when idle"**
- **"Respect Do Not Disturb / Focus"**
- (There is **no** quiet-hours feature.)

**General**

- **Day rollover time** — a single instant, default **04:00**.
- **Global day window** — start–end, default **09:00–18:00**, used as the default rotation active hours.
- **Start at login.**

### 3.7 The off-switch appears in three places

1. The **toast** Pause button.
2. The **tray** "Pause nudges".
3. The **dialog** "Turn off nudges".

---

## 4. Scheduling Model (LOCKED — the heart of the app)

This is the most important part of the build. Read it precisely.

### 4.1 Habit

A **Habit** = content + exactly **one** trigger.

Content fields:

- `name`
- `instructions`
- `media_path` (optional)
- `category` — one of `exercise` | `general`
- `enabled`

### 4.2 Trigger (exactly one per habit)

A trigger is one of:

**(a) RotationMember `{ weight }`**
The habit belongs to a **Rotation** (§4.3). `weight` biases the picker.

**(b) Schedule** — one of two forms:

- **at-time + recurrence**: a time-of-day plus a recurrence of `daily` | `weekdays` | `specific weekdays`. E.g. "09:00 daily".
- **weekly-count**: N times per week. Implement weekly-count as a **thin variant of time-of-day**: an **optional** preferred time, capped to **N auto-chosen days per week**, reusing the time-of-day slot logic.

Scheduled habits carry an **`expires_at_day_end`** flag: if not completed by the day rollover, the habit **expires** — logged as `expired`, and **NOT** carried to the next day.

### 4.3 Rotation

A **Rotation** = interval + window + members.

- **interval** — how often, e.g. every 30 minutes.
- **window** — one of:
  - **own custom window** (its own start/end),
  - **inherit the global day window**,
  - **always-on**.
- **members** — habits, each with a `weight`.

**Picker rules:**

- Weighted-random-ish, but **DETERMINISTIC in tests** (seeded/injected randomness).
- **MUST avoid showing the same drill twice in a row.**
- A lone interval habit is simply a **rotation of one**.

### 4.4 Globals

- **Day rollover point** — a single instant (default **04:00**). The day runs rollover → rollover. This is when scheduled habits **expire** and when weekly counts / daily stats **reset**. It is **NOT** about when things fire.
- **Global day window** (default **09:00–18:00**) — the default active hours a rotation borrows when it inherits.

### 4.5 Quiet rules (ALWAYS apply; gate every trigger)

Quiet rules gate **every** trigger — both rotation ticks and scheduled times.

- **Idle** (always on): don't nudge an empty chair. **Hold** and **re-arm on return.**
- **Calendar pause**: read the **local** calendar (EventKit, fully offline). Modes: **"all events"** vs **"events with ≥1 other attendee"** (**default**). A **"real meeting"** = an event happening **now** that matches the mode.
- **Do Not Disturb / Focus.**

**Deferral rule:** when a rotation tick or a scheduled time falls **inside** a quiet period, **DEFER** to the first **non-quiet** slot **after** it.

### 4.6 The pure-scheduler contract

The scheduler **MUST be a pure function**. Pure + deterministic ⇒ exhaustively unit-tested. **This is the single most important TDD target in the whole build.**

```
schedule(
    habits,             // all habits with their content + triggers
    rotations,          // all rotations with interval/window/members
    now,                // the current instant (injected, never read from the clock)
    quiet_state,        // { idle, real_meeting_now, dnd } as of `now`
    day_config,         // { rollover, global_window_start, global_window_end }
    last_shown_state,   // per-rotation last-shown habit; last-shown instants; counts
) -> Decision
```

`Decision` conveys:

- **which habit is due now** (if any), and
- the **next due time** (when to wake up next), and
- **expirations** — scheduled habits whose day has rolled over uncompleted, to be logged as `expired`.

Contract requirements:

- **No hidden inputs.** The function reads nothing from the wall clock, filesystem, RNG, or calendar directly. Everything — including `now`, quiet state, and any randomness seed — is passed in.
- **Deterministic.** Given identical inputs, it returns an identical `Decision` every time. The picker's randomness is seeded/derived from injected state so tests can assert exact outcomes.
- **Idempotent read.** Calling it does not mutate its inputs. State transitions (recording what was shown, marking done/skip/expired) happen **outside** the pure function, in the caller.
- **Quiet gating is internal.** The function applies the quiet rules and deferral logic itself, using `quiet_state`; it does not fire anything into a quiet period.

### 4.7 Worked examples

These illustrate the contract. They are normative for behaviour.

**Example A — rotation tick during a meeting defers.**
A rotation has `interval = 30 min`, inherits the global window (09:00–18:00). Last shown at 10:00. Next tick is due 10:30. At 10:30 `quiet_state.real_meeting_now = true` (a calendar event with another attendee runs 10:15–11:00). The scheduler does **not** fire at 10:30; it **defers** to the first non-quiet slot after the meeting — 11:00 (assuming idle/DND are clear then). `Decision.next_due` = 11:00.

**Example B — no drill twice in a row.**
Rotation members: Lunge-and-reach (weight 2), Glute bridges (1), Wall sit→squat (1). `last_shown_state` says the previous pick was Lunge-and-reach. Even though Lunge-and-reach has the highest weight, the picker **excludes the immediately-previous member** from this draw, then does the weighted pick among the rest. So this tick picks from {Glute bridges, Wall sit→squat}.

**Example C — scheduled habit expires at rollover.**
A habit is scheduled "09:00 daily" with `expires_at_day_end = true`. Rollover is 04:00. It fires at 09:00 but the user never acts. At the next rollover (04:00 the following day) the scheduler reports it in `Decision.expirations`; the caller logs an `expired` event. It is **not** re-shown or carried forward — the next day's 09:00 instance is a fresh occurrence.

**Example D — weekly-count reuses time-of-day slots.**
A habit is "3× / week" with an optional preferred time of 17:00. The scheduler treats it as a time-of-day slot at 17:00, but only arms it on up to **3 auto-chosen days** within the current rollover-defined week. Once 3 completions are logged in the week, no further instances arm until the week resets at rollover. If no preferred time is set, the slot logic auto-chooses a time within the global day window.

**Example E — idle holds, then re-arms.**
A rotation tick is due at 14:00 but `quiet_state.idle = true` (empty chair). The scheduler **holds** — nothing fires. When the user returns and a later `schedule(...)` call has `idle = false`, the held tick **re-arms** and fires at that first non-quiet slot.

---

## 5. Storage Schema (SQLite via rusqlite)

Column names may be adjusted sensibly during implementation; the shape below is authoritative.

**`habits`**

| Column | Notes |
| --- | --- |
| `id` | primary key |
| `name` | |
| `instructions` | |
| `media_path` | nullable |
| `category` | `exercise` \| `general` |
| `enabled` | |
| `trigger_kind` | which trigger form (rotation-member / schedule-at-time / schedule-weekly-count) |
| `trigger_config_json` | trigger-specific config (recurrence, weekdays, N, preferred time, `expires_at_day_end`) |
| `weight` | rotation-member weight (nullable for non-members) |
| `rotation_id` | FK → `rotations.id` (nullable for non-members) |
| `created_at` | |

**`rotations`**

| Column | Notes |
| --- | --- |
| `id` | primary key |
| `name` | |
| `interval_secs` | |
| `window_kind` | own / inherit-global / always-on |
| `window_start` | nullable (used when `own`) |
| `window_end` | nullable (used when `own`) |

**`events`**

| Column | Notes |
| --- | --- |
| `id` | primary key |
| `habit_id` | FK → `habits.id` |
| `action` | `done` \| `skipped` \| `snoozed` \| `expired` |
| `at` | timestamp |

**`config`** (key/value) **or** a single-row settings table with these fields:

- `day_rollover`
- `day_window_start`
- `day_window_end`
- `calendar_pause_enabled`
- `calendar_mode` (`all` \| `with-others`)
- `idle_enabled`
- `dnd_enabled`
- `start_at_login`

**Migrations:** simple — create tables if not exist. Tests use a **temp / in-memory** DB.

---

## 6. MCP Server (rmcp, in-process, LOCAL transport only)

Runs in-process via **rmcp**, on a **local transport only**. This is how a locally-running LLM manages the app. **Nothing leaves the machine.**

**Tools:**

- `add_habit`
- `list_habits`
- `update_habit`
- `disable_habit`
- `query_log`
- `log_event` (optional)

The tool surface maps directly onto the store operations (§5) and the domain model (§4). Validation must be strict: a habit has exactly one trigger; category is one of the two allowed values; failures are loud, not silent.

---

## 7. Seed Content (evidence-based movement drills)

Seed **one rotation**:

- **interval:** 30 min
- **window:** inherit the global day window
- **members** (all category `exercise`), with weights:

| Drill | Weight | Meta |
| --- | --- | --- |
| Lunge-and-reach | **2** | 5 slow reps/leg, reach overhead |
| Glute bridges | 1 | 20, or single-leg 10/side |
| Single-leg Romanian deadlift | 1 | 8/leg, 3s lower |
| 90/90 hip switches | 1 | 10 slow |
| Half-kneeling hip-flexor stretch | 1 | 45s/side |
| Light cardio snack | 1 | 3 min easy bike / step-ups |
| Wall sit 40s → deep squat hold 45s | 1 | — |
| Side-lying hip abduction | 1 | 15/side |

Plus **one loaded strength session** as a **weekly-count** habit:

- Target **3× / week**.
- Progression: bridges → hip thrusts; add a real RDL.

**Default config:** rollover **04:00**, day window **09:00–18:00**, calendar mode **"with-others"**.

---

## 8. Meeting-Aware Pause (essential, v1)

Reuse the proven local approach from the prior session.

- An **EventKit reader** of the local Calendar store — **100% local, no network**.
- Classifies a **"real meeting"** by attendee count: **≥1 other attendee** → real meeting; **solo** → focus block.
- Modes (from Settings §3.6 / config §5): **"all events"** vs **"events with someone else"** (default).
- On macOS this needs **calendar TCC permission**; an **ad-hoc-signed helper** works.
- Implement as a **Rust-callable probe**. A small Swift/objc helper invoked by the core is acceptable — **keep it local.**

The probe feeds `quiet_state.real_meeting_now` into the pure scheduler (§4.6). It performs classification only; the deferral decision lives inside the scheduler.

---

## 9. Testing Strategy

### 9.1 Unit-TDD'able (pure logic — the priority)

- **The scheduler (§4.6) is the primary TDD target.** Because it is pure and deterministic, test it exhaustively:
  - rotation ticks inside/outside the window;
  - `always-on`, `inherit-global`, and `own` windows;
  - the deferral rule across each quiet source (idle, real meeting, DND) and combinations;
  - the "no same drill twice in a row" invariant, including a rotation of one;
  - weighted-pick distribution under a fixed seed (assert exact picks);
  - scheduled at-time recurrences (`daily` / `weekdays` / `specific weekdays`);
  - weekly-count arming, the N-per-week cap, and reset at rollover;
  - `expires_at_day_end` producing `expired` at rollover with no carry-forward;
  - idle hold → re-arm on return.
  - Cover the worked examples (§4.7) as named test cases.
- **Domain model** validation (exactly one trigger; category enum; weight/rotation coupling).
- Give tests **BDD-ish names/comments**.

### 9.2 Integration-tested (impure edges)

- **Store** (§5): migrations, CRUD, event logging, weekly-count queries — against a temp/in-memory SQLite DB.
- **MCP tools** (§6): each tool end-to-end against a temp DB over the local transport.
- **Frontend** (vitest + @testing-library/svelte, jsdom): toast rendering and actions (Done/Skip/Pause), expand-to-dialog, custom-pause presets, settings controls.
- **Quiet-OS probes** (idle, DND, EventKit calendar): thin wrappers, mocked in unit tests; exercised in manual/integration checks since they require OS permissions and real state. The pure scheduler consumes their **output** (`quiet_state`), so scheduling logic never depends on live OS calls.

### 9.3 Principle

Keep the impure surface thin and push all decisions into the pure scheduler so the hard logic is covered by fast, deterministic Rust unit tests. Fail loudly — no silent fallbacks — in both code and tests.

---

## 10. Ordered Task List (mirrors the build)

Implement in this order. Each task should land with its tests.

1. **db-store** — SQLite schema + migrations (create-if-not-exist) + CRUD + event logging, via rusqlite; temp/in-memory DB in tests.
2. **domain-model** — Habit / trigger / Rotation / globals types; strict validation (exactly one trigger, category enum, weight/rotation coupling).
3. **scheduler** — the pure function (§4.6). Exhaustive unit tests first (TDD); cover all worked examples. **Highest priority.**
4. **commands** — Tauri commands wiring the store + scheduler to the frontend (due-now, act done/skip/snooze, pause/resume, stats).
5. **toast** — the corner toast surface (§3.1): always-on-top, frameless, no focus steal, no timeout, Done/Skip/Pause, click-to-expand.
6. **dialog** — the expanded dialog (§3.2): media, category pill, instructions, meta, Done/Skip/Snooze, Settings + Turn-off-nudges footer.
7. **pause-ui** — tray pause presets (§3.3), custom pause dialog with `datetime-local` + presets (§3.4), paused-state card + dimmed icon (§3.5).
8. **settings** — the Settings window (§3.6): quiet rules (calendar toggle + mode segmented control, idle, DND), general (rollover, day window, start-at-login).
9. **tray** — the macOS menu-bar menu (§3.3) with all items and wiring.
10. **quiet-os** — OS probes feeding `quiet_state`: idle detection, DND/Focus, and the EventKit meeting-aware pause helper (§8).
11. **mcp** — the rmcp in-process server (§6) with all tools, local transport only.
12. **seed-wire** — seed the movement rotation + weekly-count strength habit + default config (§7) on first run.

---

## 11. Conventions

- **British English everywhere**: code, comments, UI copy, commits, tests.
- **Small functions / modules.** Split anything large.
- **Fail loudly** — no silent fallbacks.
- **Rust:** idiomatic; `thiserror` / `anyhow` acceptable for errors; small modules.
- **TS / Svelte 5:** runes (`$state` / `$derived`), fully typed.
- **Tailwind v4** utility classes.
- **Match the existing scaffold's style.**
- Tests get **BDD-ish names / comments**.
