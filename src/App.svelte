<script lang="ts">
  import Toast from "./lib/Toast.svelte";
  import type { DueHabit } from "./lib/types";

  // Demo habit until the due-now polling loop (a later "commands" wiring
  // task) drives this from `list_due` and the real OS quiet-state probes.
  let habit = $state<DueHabit | null>({
    habit_id: 1,
    name: "Lunge-and-reach",
    instructions: "5 slow reps/leg, reach overhead",
    media_path: null,
    category: "exercise",
  });
  let paused = $state(false);
  let expanded = $state(false);

  function handleDone(habitId: number) {
    console.info(`Marked habit ${habitId} done`);
    habit = null;
  }

  function handleSkip(habitId: number) {
    console.info(`Skipped habit ${habitId}`);
    habit = null;
  }

  function handlePause() {
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
