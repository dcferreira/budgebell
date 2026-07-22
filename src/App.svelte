<script lang="ts">
  import { onMount } from "svelte";
  import CustomPauseDialog from "./lib/CustomPauseDialog.svelte";
  import Dialog from "./lib/Dialog.svelte";
  import PausedCard from "./lib/PausedCard.svelte";
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

  let habit = $state<DueHabit | null>(isToastView ? null : demoHabit);
  let paused = $state(false);
  let expanded = $state(false);

  // The expanded dialog (design spec §3.2) needs a meta line (reps/duration)
  // that `DueHabitDto` doesn't carry yet (see `types.ts`) — null until that
  // backend field lands.
  let dialogHabit = $derived<DialogHabit | null>(habit ? { ...habit, meta: null } : null);

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
        paused = false;
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

  // Once a nudge is acted on (Done/Skip/Turn off nudges) in the toast window,
  // the transparent toast window itself must be hidden too — otherwise an
  // empty see-through box is left on screen even though its content has
  // cleared. Only the toast window is ever in this view, so this never fires
  // for Settings/Stats. Hidden rather than closed, since the scheduler's
  // `ensure_toast_window` reuses and re-shows this same labelled window on
  // the next due habit.
  async function hideToastWindow() {
    if (!isToastView) {
      return;
    }
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().hide();
  }

  async function handleDone(habitId: number) {
    await invokeCommand("complete_habit", { habitId });
    habit = null;
    expanded = false;
    await hideToastWindow();
  }

  async function handleSkip(habitId: number) {
    await invokeCommand("skip_habit", { habitId });
    habit = null;
    expanded = false;
    await hideToastWindow();
  }

  async function handleSnooze(habitId: number) {
    await invokeCommand("snooze_habit", { habitId });
    habit = null;
    expanded = false;
  }

  async function handlePause() {
    await invokeCommand("pause", { durationSecs: TOAST_PAUSE_SECS });
    paused = true;
    expanded = false;
    await hideToastWindow();
  }

  function handleExpand() {
    expanded = true;
  }

  async function handleResume() {
    await invokeCommand("resume");
    paused = false;
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
    {#if paused}
      <!-- Nudges are paused (design spec §3.5); Resume re-arms the scheduler -->
      <PausedCard onResume={handleResume} />
    {:else if expanded && dialogHabit}
      <!-- Clicking the toast body expands it into the dialog (design spec §3.2) -->
      <Dialog
        habit={dialogHabit}
        onDone={handleDone}
        onSkip={handleSkip}
        onSnooze={handleSnooze}
        onSettings={handleSettings}
        onTurnOffNudges={handlePause}
      />
    {:else if habit}
      <Toast {habit} onDone={handleDone} onSkip={handleSkip} onPause={handlePause} onExpand={handleExpand} />
    {/if}
  </main>
{/if}
