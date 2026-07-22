<script lang="ts">
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import CustomPauseDialog from "./lib/CustomPauseDialog.svelte";
  import Dialog from "./lib/Dialog.svelte";
  import Settings from "./lib/Settings.svelte";
  import StatsWindow from "./lib/StatsWindow.svelte";
  import Toast from "./lib/Toast.svelte";
  import type { DialogHabit, DueHabit } from "./lib/types";

  // Each on-demand window (opened by the tray) and the scheduler's toast window
  // load this same app under a distinct `?view=` route; a plain browser (no
  // view) falls through to the demo toast so components stay dev-exercisable
  // without a running Tauri runtime.
  const view = new URLSearchParams(window.location.search).get("view");

  // The toast window (opened by the Rust scheduler tick, design spec §10) loads
  // this app with `?view=toast`. In that mode the due habit is pushed from the
  // backend; the demo window keeps a demo habit so the component is exercisable
  // without a running Tauri runtime.
  const isToastView = view === "toast";

  const demoHabit: DueHabit = {
    habit_id: 1,
    name: "Lunge-and-reach",
    instructions: "5 slow reps/leg, reach overhead",
    media_path: null,
    category: "exercise",
  };

  // A 30-minute pause when the user hits the toast's Pause button.
  const TOAST_PAUSE_SECS = 30 * 60;

  // The toast window is created at 360×230 (see runtime.rs); the compact card
  // fits that, but the expanded dialog is much taller and would otherwise be
  // crammed into it. So the window grows when expanded and shrinks back on
  // collapse (design spec §6). Width is unchanged, so the top-right anchor is
  // preserved — the window simply extends downward.
  const TOAST_WIDTH = 360;
  const TOAST_HEIGHT = 230;
  const DIALOG_HEIGHT = 460;

  let habit = $state<DueHabit | null>(isToastView ? null : demoHabit);
  let expanded = $state(false);

  // The expanded dialog (design spec §3.2) needs a meta line (reps/duration)
  // that `DueHabitDto` doesn't carry yet (see `types.ts`) — null until that
  // backend field lands.
  let dialogHabit = $derived<DialogHabit | null>(habit ? { ...habit, meta: null } : null);

  // `media_path` is now an absolute filesystem path resolved by the backend
  // (design spec §4); the webview can only load it via Tauri's asset
  // protocol, converted through `convertFileSrc`. That call needs the Tauri
  // runtime, absent under vitest/jsdom, so it's gated behind `isToastView`
  // and kept out of the leaf components (design spec §4.2).
  function toMediaUrl(path: string): string {
    return isToastView ? convertFileSrc(path) : path;
  }

  let mediaUrl = $derived(habit?.media_path ? toMediaUrl(habit.media_path) : null);

  // Resize the toast window to fit whichever surface is showing (design spec
  // §6). Only the toast window (this Tauri view) is ever resized; the lazy
  // import keeps the window API out of non-Tauri test/demo renders.
  $effect(() => {
    if (!isToastView) {
      return;
    }
    const height = expanded ? DIALOG_HEIGHT : TOAST_HEIGHT;
    void getCurrentWindow().setSize(new LogicalSize(TOAST_WIDTH, height));
  });

  onMount(() => {
    if (!isToastView) {
      return;
    }
    let unlisten: (() => void) | undefined;
    void (async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      const { listen } = await import("@tauri-apps/api/event");
      // Fetch whatever is due now, so a window that opened after the tick's
      // push event still renders the current nudge.
      habit = await invoke<DueHabit | null>("current_due");
      unlisten = await listen<DueHabit>("habit-due", (event) => {
        habit = event.payload;
        expanded = false;
      });
    })();
    return () => unlisten?.();
  });

  async function invokeCommand(command: string, args?: Record<string, unknown>) {
    if (!isToastView) {
      return;
    }
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke(command, args);
  }

  // Once a nudge is acted on (Done/Skip/Snooze/Pause) in the toast window, the
  // transparent toast window itself must be closed too — otherwise an empty
  // see-through box (and its OS drop-shadow) is left lingering on screen even
  // though its content has cleared. Closed rather than hidden so nothing can
  // linger; the scheduler's `ensure_toast_window` rebuilds a fresh window on
  // the next due habit. Only the toast window is ever in this view, so this
  // never fires for Settings/Stats.
  async function closeToastWindow() {
    if (!isToastView) {
      return;
    }
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().close();
  }

  async function handleDone(habitId: number) {
    await invokeCommand("complete_habit", { habitId });
    habit = null;
    expanded = false;
    await closeToastWindow();
  }

  async function handleSkip(habitId: number) {
    await invokeCommand("skip_habit", { habitId });
    habit = null;
    expanded = false;
    await closeToastWindow();
  }

  async function handleSnooze(habitId: number) {
    await invokeCommand("snooze_habit", { habitId });
    habit = null;
    expanded = false;
    await closeToastWindow();
  }

  // Option A (menu-bar style): pausing dismisses the toast just like Done/Skip,
  // rather than showing an in-window paused card. The pause is resumable from
  // the tray's "Resume nudges" item.
  async function handlePause() {
    await invokeCommand("pause", { durationSecs: TOAST_PAUSE_SECS });
    habit = null;
    expanded = false;
    await closeToastWindow();
  }

  function handleExpand() {
    expanded = true;
  }

  // The dialog's back/collapse control (design spec §6) returns to the toast
  // without acting on the habit, mirroring the footer Settings link's collapse.
  function handleCollapse() {
    expanded = false;
  }

  // The Settings window is opened by the tray today (design spec §3.3); the
  // "settings"/"tray" tasks (§10) wire a direct path from here. Until then,
  // following the footer link just collapses back to the toast.
  function handleSettings() {
    expanded = false;
  }

  // The backend `pause` command's `until` parameter is a chrono `NaiveDateTime`,
  // deserialised from an ISO-8601 wall-clock string with no timezone suffix.
  // The datetime-local picker already gives local wall-clock time, so we format
  // the chosen instant with its local components (not a UTC/`toISOString`
  // conversion, which would shift the hour).
  function toNaiveLocalString(date: Date): string {
    const pad = (value: number) => String(value).padStart(2, "0");
    return (
      `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}` +
      `T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`
    );
  }

  // Closes the current on-demand window (Custom pause) via the Tauri window API.
  async function closeThisWindow() {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().close();
  }

  async function handleCustomPause(resumeAt: Date) {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("pause", { until: toNaiveLocalString(resumeAt) });
    await closeThisWindow();
  }
</script>

{#if view === "settings"}
  <Settings />
{:else if view === "stats"}
  <StatsWindow />
{:else if view === "custom-pause"}
  <main class="flex min-h-screen items-center justify-center p-4">
    <CustomPauseDialog onCancel={closeThisWindow} onPause={handleCustomPause} />
  </main>
{:else}
  <main class="flex min-h-screen items-start justify-end p-4">
    {#if expanded && dialogHabit}
      <!-- Clicking the toast body expands it into the dialog (design spec §3.2) -->
      <Dialog
        habit={dialogHabit}
        {mediaUrl}
        onDone={handleDone}
        onSkip={handleSkip}
        onSnooze={handleSnooze}
        onSettings={handleSettings}
        onTurnOffNudges={handlePause}
        onCollapse={handleCollapse}
      />
    {:else if habit}
      <Toast
        {habit}
        {mediaUrl}
        onDone={handleDone}
        onSkip={handleSkip}
        onPause={handlePause}
        onExpand={handleExpand}
      />
    {/if}
  </main>
{/if}
