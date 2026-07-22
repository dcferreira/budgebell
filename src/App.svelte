<script lang="ts">
  import { onMount } from "svelte";
  import Dialog from "./lib/Dialog.svelte";
  import PausedCard from "./lib/PausedCard.svelte";
  import Toast from "./lib/Toast.svelte";
  import type { DialogHabit, DueHabit } from "./lib/types";

  // The toast window (opened by the Rust scheduler tick, design spec §10) loads
  // this app with `?view=toast`. In that mode the due habit is pushed from the
  // backend; the default window keeps a demo habit so the component is
  // exercisable without a running Tauri runtime.
  const isToastView =
    new URLSearchParams(window.location.search).get("view") === "toast";

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

  async function handleDone(habitId: number) {
    await invokeCommand("complete_habit", { habitId });
    habit = null;
    expanded = false;
  }

  async function handleSkip(habitId: number) {
    await invokeCommand("skip_habit", { habitId });
    habit = null;
    expanded = false;
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
</script>

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
