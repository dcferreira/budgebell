<script lang="ts">
  import { onMount } from "svelte";
  import Toast from "./lib/Toast.svelte";
  import type { DueHabit } from "./lib/types";

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
  }

  async function handleSkip(habitId: number) {
    await invokeCommand("skip_habit", { habitId });
    habit = null;
  }

  async function handlePause() {
    await invokeCommand("pause", { durationSecs: TOAST_PAUSE_SECS });
    paused = true;
  }

  function handleExpand() {
    expanded = true;
  }
</script>

<main class="flex min-h-screen items-start justify-end p-4">
  {#if paused}
    <p class="text-sm text-gray-500">Nudges paused</p>
  {:else if habit}
    <Toast {habit} onDone={handleDone} onSkip={handleSkip} onPause={handlePause} onExpand={handleExpand} />
  {/if}

  {#if expanded}
    <!-- The expanded dialog (design spec §3.2) lands in a later task. -->
    <p class="absolute top-4 left-4 text-sm text-gray-500">Expanded dialog coming soon</p>
  {/if}
</main>
