# habits — media rendering in nudges (design spec)

Date: 2026-07-22
Status: approved (brainstormed with the user)
Scope: render habit images in the toast + dialog; fix the toast layout; add a
dialog collapse control. **Image-only this session** (video deferred but not
removed). Continues the shipped build (`docs/DESIGN.md`).

> Inherits the app-wide **LOCAL ONLY** guarantee (`DESIGN.md` intro): no
> outbound network, ever. This spec keeps it intact — media is read only from a
> local, tightly-scoped folder.

## 1. Problem

The domain model already carries `media_path`, and `Toast`/`Dialog` already
attempt `<img src={habit.media_path}>` — but **no image ever renders**, for two
reasons:

1. **The asset plumbing is absent.** The webview loads from
   `http://localhost:1420`; a bare filesystem path in `src` cannot be reached by
   WKWebView. Tauri's asset protocol is not enabled (`tauri.conf.json` has only
   `csp: null`, no `assetProtocol`), and nothing calls `convertFileSrc`.
2. **No habit has media.** The seed sets `media_path: None` everywhere, so the
   only thing ever shown is the placeholder icon.

Two UX faults compound this:

- **The toast is cramped.** Card is `w-[264px]`; the instruction line is a
  single-line `truncate` ("5 slow reps/leg, rea…").
- **The Pause button collides with the title.** It is absolute top-right, in the
  exact space the title occupies; long names ("Half-kneeling hip-flexor stretch")
  run into it.
- **The expanded dialog is a dead end.** Expand is frontend-only state
  (`expanded` swaps `Toast`↔`Dialog` in the one window); there is no
  `onCollapse` and the dialog has no back control.

## 2. Goals / non-goals

**Goals**
- Real local images render in both the toast thumbnail and the dialog banner.
- Toast layout reworked: bigger thumbnail, two-line instructions, relocated Pause.
- A collapse/back control returns the dialog to the toast.
- Media read only from a local, scoped folder — LOCAL ONLY preserved.

**Non-goals (this session)**
- Video (extension-classification code stays, but not exercised/verified).
- Remote image URLs (see §7 follow-up: download-once-into-folder cache).
- Window resize-on-expand (verify clipping live first — §6).
- A media picker / import UI (files are placed in the folder by hand for now).
- Bundled sample media shipped with the app.

## 3. Media storage & scope (decided)

- **Media folder:** `<app_data_dir>/media`, i.e.
  `~/Library/Application Support/com.dcferreira.habits/media` on macOS. Created
  on startup if absent.
- **`media_path` is a relative filename** within that folder (e.g.
  `"lunge.png"`), not an absolute path. This is what the DB stores and what MCP
  `add_habit`/`update_habit` accept.
- **Asset-protocol scope** (`tauri.conf.json` → `app.security.assetProtocol`):
  ```json
  "assetProtocol": { "enable": true, "scope": ["$APPDATA/media/**"] }
  ```
  `$APPDATA` resolves to the app-data dir above, so the webview may read **only**
  files under `.../com.dcferreira.habits/media`. CSP is `null`, so no `img-src`
  restriction blocks the asset scheme. No capability change is required (asset
  access is governed by the config scope, not a permission string).

## 4. Resolution & rendering data flow

The pure scheduler does not know the media dir, so resolution happens at the
**IPC boundary**, and URL conversion happens in `App.svelte` so the leaf
components stay dumb and unit-testable.

```
DB (relative "lunge.png")
  → command layer (has app_data_dir): resolve_media(dir, "lunge.png")
        → Some(absolute PathBuf) | None   [pure helper, Rust-TDD'd]
  → DueHabitDto.media_path = Some(absolute string) | None
  → App.svelte: mediaUrl = media_path ? convertFileSrc(media_path) : null
  → <Toast mediaUrl={…}/> / <Dialog mediaUrl={…}/>  render <img src={mediaUrl}>
```

### 4.1 Rust — `resolve_media` (pure, TDD)

`fn resolve_media(media_dir: &Path, media_path: &str) -> Option<PathBuf>`

- Joins `media_dir` + `media_path`.
- **Rejects path traversal**: if the normalised result escapes `media_dir`
  (e.g. `../../secrets`), return `None`. (Local-only personal app, but a cheap,
  well-bounded invariant and a clean red-green case.)
- Returns the absolute path. Existence is **not** checked here (keep it pure and
  filesystem-free for tests); a missing file simply fails to load in the webview
  and falls back to the placeholder — acceptable for v1.
- Empty / `None` `media_path` → `None`.

Resolution is applied where `DueHabitDto` is built for IPC (the `current_due` /
`list_due` command path), which has the `AppHandle` → `app_data_dir()`. The
`From<DueHabit>` impl stays, but the command layer maps `media_path` through
`resolve_media` with the media dir before returning. `DueHabitDto.media_path`
therefore becomes the **absolute resolved path** (or `None`).

### 4.2 Frontend — URL conversion in `App.svelte`

- `App.svelte` derives `mediaUrl` from `habit.media_path`:
  `media_path ? convertFileSrc(media_path) : null`.
- `convertFileSrc` (from `@tauri-apps/api/core`) needs the Tauri runtime, which
  is absent under vitest/jsdom — so it is called **only** in `App.svelte` (which
  already gates on `isToastView` and imports Tauri lazily), never in leaf
  components. Component tests pass a plain string for `mediaUrl`.
- `Toast` and `Dialog` gain a `mediaUrl: string | null` prop and render
  `<img src={mediaUrl}>` / `<video src={mediaUrl}>` from it, keeping the
  `media_path`-null → placeholder branch.

## 5. Layout rework (Toast.svelte)

Chosen in brainstorming, previewed in the mockup artefact:

- **Card width** 264 → **300 px**.
- **Thumbnail** 52 → **88 px**, `object-cover`, rounded.
- **Instructions** single-line `truncate` → **two-line clamp** (`line-clamp-2`),
  fixing the truncation for the no-image case too.
- **Pause** moves from a top-right text button to a **24 px circular icon-chip**
  top-right: transparent fill, `border-border` outline, `text-ink-soft` ⏸ glyph,
  `bg-surface-2` on hover — the Skip button's ghost treatment, scaled down. The
  card body grid gets right padding (~26 px) so the title never reaches the chip.
- **Placeholder** (no media): the existing gradient figure, sized to the 88 px
  thumbnail.
- Footer hint stays ("Click card for details").

## 6. Dialog collapse (Dialog.svelte + App.svelte)

- `Dialog` gains an **`onCollapse` prop** and a back/collapse control (a small
  top-left back affordance in the dialog header).
- `App.svelte` adds `handleCollapse() { expanded = false }` and passes it as
  `onCollapse`.
- `Dialog` renders the image/video banner from `mediaUrl` (extension-classified
  for video via the existing `videoExtensionPattern`; video not verified this
  session).
- **Window resize (implemented after live verification).** The live run
  confirmed the dialog is crammed into the compact toast window, so the deferred
  resize was brought into scope: the toast window is created at 360×230
  (`runtime.rs`), and an `$effect` in `App.svelte` grows it to 360×460 on expand
  and shrinks it back on collapse. Width is unchanged so the top-right anchor is
  preserved (the window extends downward, over transparent space). This needs
  the `core:window:allow-set-size` capability.

## 7. Testing & follow-ups

- **Live media test:** drop an image into `.../com.dcferreira.habits/media`, set
  a habit's `media_path` to that filename (via MCP `add_habit`/`update_habit`, or
  a temporary DB edit — settle at build), run `pnpm tauri dev`, confirm it
  renders in the toast and dialog.
- **Follow-ups (noted, not built):** URL convenience via *download-once into the
  media folder* then render from disk (keeps LOCAL ONLY); window resize-on-expand;
  a media import/picker UI; optional bundled sample media on first run.

## 8. Test plan (red-green where a pure unit exists)

- **Rust unit (`resolve_media`)**: relative filename → absolute under media dir;
  `../` traversal → `None`; empty/None → `None`.
- **Frontend component (`Toast.test.ts`)**: given `mediaUrl`, renders `<img>`
  with that src and the 88 px thumbnail; given null, renders the placeholder;
  two-line clamp class present; circular Pause chip present and calls `onPause`.
- **Frontend component (`Dialog.test.ts`)**: collapse control calls `onCollapse`;
  image vs placeholder branch by `mediaUrl`.
- **Frontend (`App.test.ts`)**: expand → dialog → collapse returns to toast.
- **Integration (live, user-driven)**: asset protocol serves an image end-to-end;
  dialog clipping observed to decide the resize follow-up.

## 9. Files touched

- `src-tauri/tauri.conf.json` — enable `assetProtocol` with the media scope.
- `src-tauri/src/…` — media dir creation on startup; `resolve_media` pure helper
  (new small module) + tests; apply it in the `current_due`/`list_due` command
  path.
- `src/App.svelte` — `mediaUrl` derivation via `convertFileSrc`; `handleCollapse`.
- `src/lib/Toast.svelte` — layout rework; `mediaUrl` prop.
- `src/lib/Dialog.svelte` — `mediaUrl` prop; `onCollapse` + back control.
- `src/lib/types.ts` — no shape change to `media_path` (still `string | null`;
  now carries the absolute resolved path from the backend).
- Tests alongside each.
