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
  class="relative flex w-[264px] flex-col gap-2 rounded-card border border-border bg-surface p-3 font-sans shadow-card"
  aria-label="{habit.name} nudge"
>
  <button
    class="absolute top-2 right-2 rounded-md px-1.5 py-1 text-xs font-semibold text-ink-soft hover:bg-surface-2 hover:text-ink"
    type="button"
    onclick={onPause}
  >
    Pause
  </button>

  <button
    class="grid grid-cols-[52px_1fr] items-center gap-3 rounded-md pr-8 text-left hover:bg-surface-2"
    type="button"
    aria-label="Show details for {habit.name}"
    onclick={onExpand}
  >
    {#if habit.media_path}
      <img class="h-[52px] w-[52px] shrink-0 rounded-lg object-cover" src={habit.media_path} alt={habit.name} />
    {:else}
      <div
        class="flex h-[52px] w-[52px] shrink-0 items-center justify-center rounded-lg bg-linear-to-br from-accent to-accent-ink text-white"
        aria-hidden="true"
      >
        <svg
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="1.6"
          stroke-linecap="round"
          stroke-linejoin="round"
          class="h-7 w-7"
        >
          <circle cx="12" cy="4.5" r="2" />
          <path d="M12 7v6m0 0-4 5m4-5 4 5M6 9l6 1 6-1" />
        </svg>
      </div>
    {/if}
    <div class="min-w-0">
      <p class="truncate font-display text-[1.02rem] font-semibold text-ink">{habit.name}</p>
      <p class="truncate text-sm text-ink-soft">{habit.instructions}</p>
    </div>
  </button>

  <div class="flex gap-1.5">
    <button
      class="flex-1 rounded-lg bg-accent py-1.5 text-sm font-semibold text-white hover:brightness-105"
      type="button"
      onclick={() => onDone(habit.habit_id)}
    >
      Done
    </button>
    <button
      class="flex-1 rounded-lg border border-border py-1.5 text-sm font-semibold text-ink-soft hover:bg-surface-2"
      type="button"
      onclick={() => onSkip(habit.habit_id)}
    >
      Skip
    </button>
  </div>

  <p class="text-center text-[0.68rem] text-ink-soft">Click card for details &amp; video</p>
</section>
