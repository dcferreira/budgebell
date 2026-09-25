# Budgebell — Finalised Design Specification

**Status:** Locked. Source of truth for the build.
**Date:** 2026-07-22
**Scope:** A fully-local macOS desktop app that nudges movement/exercise and general habits.

This document is self-contained: a build team can implement from it without any prior conversation. It expands the locked design into implementable sections. Do not invent features beyond what is written here, and do not re-open the decisions recorded below.

---

## 1. Overview & Local-Only Guarantee

Budgebell is a macOS desktop application that periodically nudges the user to perform short movement/exercise drills and general habits. Each nudge shows what to do (text plus an optional image or video), lets the user mark it **Done** or **Skip**, and logs the outcome. Nudges are surfaced through a single UI surface — a corner toast — that can expand into a richer dialog.

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
- **No "Start" button.** The one-click Done flow is preserved: the user reads the drill, does it, then clicks **Done**. Duration is measured implicitly (§3.8), never by an explicit start action.

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
- **"Don't nudge when the microphone is in use"** (**DEFAULT: on**) — a proxy for an ongoing (possibly ad-hoc, off-calendar) call.
- (There is **no** quiet-hours feature.)

**General**

- **Day rollover time** — a single instant, default **04:00**.
- **Global day window** — start–end, default **09:00–18:00**, used as the default rotation active hours.
- **Start at login.**

### 3.7 The off-switch appears in three places

1. The **toast** Pause button.
2. The **tray** "Pause nudges".
3. The **dialog** "Turn off nudges".

### 3.8 Duration model (how long a drill took)

Duration is measured **implicitly**, with **no "Start" button** — the one-click Done flow (§3.1) is preserved.

- Duration is the interval from the moment the **toast is shown** (`shown_at`) to the moment **Done** is clicked (`done_at`): `duration = done_at − shown_at`.
- `shown_at` is captured by the runtime when it opens the toast window; `done_at` is the timestamp of the Done action.
- This measure is only **honest** because of the **idle-withdraw** rule (§4.5): the toast is never left standing while the user is away, so the elapsed time genuinely reflects the user being present and doing the drill. Were idle nudges left on screen, the interval would balloon and mean nothing.
- Only **`done`** events carry a meaningful duration. **`skipped`** events also happen after the toast was shown — recording their `shown_at` is fine, but a skip duration is not meaningful and is not surfaced as movement time.
- `snoozed` and `expired` events carry no duration.

See §4.5 for how the runtime holds the current due occurrence's `shown_at`, and §5 for how it is stored.

### 3.9 Stats window (a real window)

Opened from the tray's **"Today's stats"** (§3.3) and the dialog footer's stats affordance. Titled **"Budgebell — Activity"**. Design finalised via mock. It aggregates **client-side** from a date-ranged log query (§6.1); it holds no logic of its own beyond rendering.

**Date bar (top).**

- **Prev (`<`) / next (`>`)** arrows around a centre label.
- Centre label shows the **weekday + date** plus a **relative tag**: *Today* / *Yesterday* / *N days ago*.
- **Forward is DISABLED when on today** — there are no future days.
- **Defaults to today.**
- The active "day" is bounded by the **day rollover** (§4.4), not midnight, so the window matches how the rest of the app counts a day.

**Summary strip (4 tiles).**

| Tile | Value | Colour |
| --- | --- | --- |
| **Done** | count of `done` events in the day | green |
| **Skipped** | count of `skipped` events in the day | amber |
| **Moving** | total **measured movement time** = **sum of `done` durations** (§3.8), formatted e.g. `24 min` or `1h 05m` | — |
| **Adherence** | `% = done / (done + skipped)`, shown as **0** if there were none | — |

**Longest sit (the signature stat).**

- The **largest gap between movements** that day, with the **time window it spanned**, e.g. `2h 31m · 12:05–14:35`.
- Computed as the **max gap between consecutive `done` events only** — the day window (§3.6) is a per-rotation scheduling default, never a fabricated leading or trailing edge here; the sole open-ended edge allowed is `last movement → now`, and only for **today**.
- With fewer than two `done` events (and, for a past day, no trailing "now" edge to fall back on), there is no meaningful sit to report.

**Activity list (single, chronological).**

- `done` and `skipped` events **merged and sorted by time**.
- Rows render differently by action:
  - **done:** a **filled green status dot**, the **time**, the **drill name** (+ a **category chip**), and the **green duration** (e.g. `1m 48s`).
  - **skipped:** a **hollow amber ring**, the **time**, the **drill name struck-through / dimmed**, and an amber **`SKIPPED`** label.
- **List header:** `N drills · X done · Y skipped`.

**Footnote (idle rule made explicit).**

- A line stating that **drills you were away for aren't shown — they are withdrawn and never logged** (§4.5). This explains any perceived "gaps" without implying missed nudges were skips.

**Visual style.** Match the existing app: the **eucalyptus / pine palette**, **Iowan display** headings + **system body** fonts, **Tailwind v4** utilities. Support **light and dark**.

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

- **Idle** (always on): don't nudge an empty chair. See the **idle-withdraw-and-discard** rule below.
- **Calendar pause**: read the **local** calendar (EventKit, fully offline). Modes: **"all events"** vs **"events with ≥1 other attendee"** (**default**). A **"real meeting"** = an event happening **now** that matches the mode.
- **Do Not Disturb / Focus.**
- **Microphone in use** (default on): a **local** CoreAudio query of `kAudioDevicePropertyDeviceIsRunningSomewhere` on the default input device (`kAudioHardwarePropertyDefaultInputDevice`). This is a device-property read, **not** audio capture, so it needs **no microphone TCC permission** and triggers **no permission prompt**. An in-use mic is a proxy for an ongoing (possibly ad-hoc, off-calendar) call.

**Deferral rule:** when a rotation tick or a scheduled time falls **inside** a quiet period (real meeting or DND), **DEFER** to the first **non-quiet** slot **after** it.

#### Idle-withdraw-and-discard (Stretchly-style)

Idle behaves differently from the meeting/DND deferral above. It **discards** the occurrence rather than logging anything, because a drill the user was never present for is not a genuine event.

- **Idle when a drill comes due:** the toast is **NOT** shown, and the occurrence is **discarded as if it never fired** — **no event is logged** (not `skipped`, not `expired`). The scheduler simply **re-arms for the next opportunity**.
- **Goes idle while the toast is showing:** the toast is **withdrawn** and the occurrence is likewise **discarded** — **no event logged**.
- **Net effect:** the only rows that ever reach the events log are **genuine user actions** (`done` / `skipped`) taken **while present**. Idle-time nudges leave **no trace**. This is what makes the duration measure (§3.8) honest — a standing toast never accrues idle time.

This refines the already-built runtime and quiet rules: the pure scheduler still surfaces the occurrence and its `next_due`; the **runtime** is responsible for withdrawing/discarding on idle and re-arming, and for capturing `shown_at` when it does open the toast.

**Current-due-occurrence state (runtime):** when the runtime opens the toast, it records the occurrence — including its `shown_at` — as the **current due occurrence** in `AppState`. The `complete_habit` / `skip_habit` commands read that `shown_at` to compute and log duration (§3.8, §5). When an occurrence is discarded on idle (either not shown, or withdrawn), the runtime clears the current due occurrence and logs nothing.

### 4.6 The pure-scheduler contract

The scheduler **MUST be a pure function**. Pure + deterministic ⇒ exhaustively unit-tested. **This is the single most important TDD target in the whole build.**

```
schedule(
    habits,             // all habits with their content + triggers
    rotations,          // all rotations with interval/window/members
    now,                // the current instant (injected, never read from the clock)
    quiet_state,        // { idle, real_meeting_now, dnd, mic_in_use } as of `now`
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

**Example E — idle discards, then re-arms (no event logged).**
A rotation tick is due at 14:00 but `quiet_state.idle = true` (empty chair). The toast is **NOT** shown and the occurrence is **discarded** — **nothing is logged** (§4.5). The scheduler **re-arms for the next opportunity**; when the user returns and a later `schedule(...)` call has `idle = false`, the next tick fires normally at the first non-quiet slot. Contrast with the meeting/DND cases (Example A), which **defer** the same slot rather than discard it.

**Example F — toast withdrawn on going idle mid-drill.**
A drill fires at 14:00 (`shown_at = 14:00`) and the toast is showing. At 14:01 the user goes idle before acting. The runtime **withdraws** the toast and **discards** the occurrence — **no `skipped` and no `expired` event** — and clears the current due occurrence in `AppState`. Because nothing was logged, no duration is recorded. The next tick re-arms as usual.

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

The events table must capture enough to compute duration (§3.8): the moment the toast was shown and the moment the action was taken.

| Column | Notes |
| --- | --- |
| `id` | primary key |
| `habit_id` | FK → `habits.id` |
| `action` | `done` \| `skipped` \| `snoozed` \| `expired` |
| `at` | **action time** — when the action was taken (`done_at` / `acted_at`). Existing column, retained. |
| `shown_at` | **new** — when the toast was shown. Nullable to keep old rows valid. For `done`, `duration = at − shown_at`. For `skipped` it is recorded but duration is not meaningful; for `snoozed` / `expired` it may be null. |

A `duration_secs` column may be stored instead of, or in addition to, `shown_at` if the implementer prefers a materialised value — but `shown_at` is the authoritative source and should be preferred so aggregation can recompute cleanly.

Recall (§4.5) that idle occurrences are **discarded and never written**, so every row here is a genuine present-user action.

**`config`** (key/value) **or** a single-row settings table with these fields:

- `day_rollover`
- `day_window_start`
- `day_window_end`
- `calendar_pause_enabled`
- `calendar_mode` (`all` \| `with-others`)
- `idle_enabled`
- `dnd_enabled`
- `mic_pause_enabled` (**default: on**)
- `start_at_login`

**Migrations:** simple — create tables if not exist. The `events.shown_at` addition needs an **idempotent migration** so existing databases upgrade cleanly: either a guarded `ALTER TABLE events ADD COLUMN shown_at …` (skip if the column already exists, e.g. by inspecting `PRAGMA table_info(events)`) or a small **versioned migration** keyed off a `user_version` / schema-version marker. Re-running the migration on an already-upgraded DB must be a no-op. Tests use a **temp / in-memory** DB, and must cover upgrading a pre-`shown_at` DB.

---

## 6. MCP Server (rmcp, in-process, LOCAL transport only)

Runs in-process via **rmcp**, on a **local transport only**. This is how a locally-running LLM manages the app. **Nothing leaves the machine.**

**Tools:**

- `add_habit`
- `disable_habit`
- `day_log` / `query_log` — the **date-ranged log** (see §6.1)
- `list_habits`
- `log_event` (optional)
- `update_habit`

The tool surface maps directly onto the store operations (§5) and the domain model (§4). Validation must be strict: a habit has exactly one trigger; category is one of the two allowed values; failures are loud, not silent.

### 6.1 Stats data path (shared by the Stats window and MCP)

The Stats window (§3.9) does **not** compute in Rust-per-window nor in bespoke UI logic — it aggregates **client-side** from a **date-ranged log query**, and the heavy lifting lives in **pure Rust** so it is unit-tested once and reused.

- **Date-ranged query.** A Tauri command exposes the events for a range — e.g. `query_log(from, to)` or a convenience `day_log(date)` that resolves the range from the given day using the **day rollover** (§4.4). It returns the merged `done` / `skipped` (and other) events with their `at`, `shown_at`, habit name and category — enough for the window to render §3.9 without further round-trips.
- **Pure aggregation helpers (unit-tested).** The aggregations are **pure Rust functions** — no clock, no DB inside them — taking the fetched events (plus `now`, only to resolve the today-only trailing edge) and returning:
  - **day summary** (done count, skipped count, total moving time, adherence %),
  - **adherence** (`done / (done + skipped)`, `0` when none),
  - **longest sedentary gap** (max gap between consecutive `done` events only — never the day window, per §3.9).
  These are tested exhaustively (empty day, all skipped, single movement, today's trailing edge) and then **exposed via the command** so the window and any caller share one implementation.
- **MCP exposure.** The same date-ranged log / day-summary is exposed as an **MCP tool** (extending this rmcp server) so a **locally-running LLM can read adherence** and the daily picture. **Local transport only — nothing leaves the machine** (§1).

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
- **Stats aggregation helpers (§6.1)** — pure functions, so unit-test exhaustively: day summary counts, total moving time from `done` durations, adherence (including the `0`-when-none case), and the **longest sedentary gap** with its edge cases (empty day and single-movement-on-a-past-day both yield no gap, never a fabricated day-window edge; today's single trailing "last movement → now" gap).
- Give tests **BDD-ish names/comments**.

### 9.2 Integration-tested (impure edges)

- **Store** (§5): migrations (including the **idempotent `shown_at` upgrade** applied to a pre-`shown_at` DB, and a re-run no-op), CRUD, event logging with `shown_at`, date-ranged log queries (§6.1), weekly-count queries — against a temp/in-memory SQLite DB.
- **MCP tools** (§6): each tool end-to-end against a temp DB over the local transport, including the date-ranged log / day-summary tool.
- **Frontend** (vitest + @testing-library/svelte, jsdom): toast rendering and actions (Done/Skip/Pause), expand-to-dialog, custom-pause presets, settings controls, and the **Stats window** (§3.9) — date-bar navigation with forward disabled on today, summary tiles, longest-sit rendering, and the merged done/skipped activity list.
- **Quiet-OS probes** (idle, DND, EventKit calendar): thin wrappers, mocked in unit tests; exercised in manual/integration checks since they require OS permissions and real state. The pure scheduler consumes their **output** (`quiet_state`), so scheduling logic never depends on live OS calls.

### 9.3 Principle

Keep the impure surface thin and push all decisions into the pure scheduler so the hard logic is covered by fast, deterministic Rust unit tests. Fail loudly — no silent fallbacks — in both code and tests.

---

## 10. Ordered Task List (mirrors the build)

Implement in this order. Each task should land with its tests.

1. **db-store** — SQLite schema + migrations (create-if-not-exist, plus the **idempotent `events.shown_at` upgrade**, §5) + CRUD + event logging with `shown_at` + date-ranged log queries (§6.1), via rusqlite; temp/in-memory DB in tests.
2. **domain-model** — Habit / trigger / Rotation / globals types; strict validation (exactly one trigger, category enum, weight/rotation coupling).
3. **scheduler** — the pure function (§4.6). Exhaustive unit tests first (TDD); cover all worked examples. **Highest priority.**
4. **commands** — Tauri commands wiring the store + scheduler to the frontend (due-now, act done/skip/snooze, pause/resume). Includes capturing `shown_at` as the **current due occurrence** in `AppState` (§4.5) and reading it in `complete_habit` / `skip_habit` to log duration (§3.8), and the **idle-withdraw-and-discard** runtime behaviour (§4.5): withdraw the toast and discard without logging when the user goes idle.
5. **stats-data** — the **pure Rust aggregation helpers** (§6.1: day summary, adherence, longest sedentary gap), unit-tested exhaustively, exposed via the date-ranged log / day-summary Tauri command.
6. **toast** — the corner toast surface (§3.1): always-on-top, frameless, no focus steal, no timeout, Done/Skip/Pause, click-to-expand, **no Start button** (§3.8).
7. **dialog** — the expanded dialog (§3.2): media, category pill, instructions, meta, Done/Skip/Snooze, Settings + Turn-off-nudges footer.
8. **stats-window** — the **Stats window** (§3.9): date bar (forward disabled on today), 4-tile summary strip, longest-sit, merged done/skipped activity list, idle footnote; aggregates client-side from the §6.1 command; eucalyptus/pine palette, Iowan display + system body fonts, light + dark.
9. **pause-ui** — tray pause presets (§3.3), custom pause dialog with `datetime-local` + presets (§3.4), paused-state card + dimmed icon (§3.5).
10. **settings** — the Settings window (§3.6): quiet rules (calendar toggle + mode segmented control, idle, DND), general (rollover, day window, start-at-login).
11. **tray** — the macOS menu-bar menu (§3.3) with all items and wiring (including "Today's stats" opening §3.9).
12. **quiet-os** — OS probes feeding `quiet_state`: idle detection, DND/Focus, and the EventKit meeting-aware pause helper (§8).
13. **mcp** — the rmcp in-process server (§6) with all tools, local transport only, including the date-ranged log / day-summary tool (§6.1).
14. **seed-wire** — seed the movement rotation + weekly-count strength habit + default config (§7) on first run.

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
