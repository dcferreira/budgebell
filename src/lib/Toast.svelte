<script lang="ts">
  // The corner toast (design spec §3.1) — the app's single nudge surface.
  // Always-on-top/frameless/never-steals-focus are window-level concerns
  // handled elsewhere; this component only renders the card and wires the
  // mouse-only interactions. There are no keyboard shortcuts anywhere.
  import type { DueHabit } from "./types";

  interface Props {
    /** The habit currently due, to nudge the user about. */
    habit: DueHabit;
    /** Called with the habit id when the user marks it done. */
    onDone: (habitId: number) => void;
    /** Called with the habit id when the user skips it. */
    onSkip: (habitId: number) => void;
    /** Called when the user pauses nudges from the toast. */
    onPause: () => void;
    /** Called when the user clicks the card body to expand into the dialog. */
    onExpand: () => void;
  }

  let { habit, onDone, onSkip, onPause, onExpand }: Props = $props();
</script>

<section
  class="relative flex w-72 flex-col gap-2 rounded-lg border border-gray-200 bg-white p-3 shadow-lg"
  aria-label="{habit.name} nudge"
>
  <button
    class="absolute top-1 right-1 rounded px-1.5 py-0.5 text-xs text-gray-400 hover:bg-gray-100 hover:text-gray-600"
    type="button"
    onclick={onPause}
  >
    Pause
  </button>

  <button
    class="flex items-center gap-3 rounded pr-8 text-left hover:bg-gray-50"
    type="button"
    aria-label="Show details for {habit.name}"
    onclick={onExpand}
  >
    {#if habit.media_path}
      <img class="h-12 w-12 shrink-0 rounded object-cover" src={habit.media_path} alt={habit.name} />
    {:else}
      <div class="h-12 w-12 shrink-0 rounded bg-gray-100" aria-hidden="true"></div>
    {/if}
    <div class="min-w-0">
      <p class="truncate font-medium text-gray-900">{habit.name}</p>
      <p class="truncate text-sm text-gray-500">{habit.instructions}</p>
    </div>
  </button>

  <div class="flex gap-2">
    <button
      class="flex-1 rounded bg-blue-600 py-1.5 text-sm font-medium text-white hover:bg-blue-700"
      type="button"
      onclick={() => onDone(habit.habit_id)}
    >
      Done
    </button>
    <button
      class="flex-1 rounded border border-gray-300 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-50"
      type="button"
      onclick={() => onSkip(habit.habit_id)}
    >
      Skip
    </button>
  </div>
</section>
