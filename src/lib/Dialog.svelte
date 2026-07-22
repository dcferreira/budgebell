<script lang="ts">
  // The expanded dialog (design spec §3.2) — reached by clicking the toast
  // body. Shows the full drill content and the remaining actions (Snooze,
  // on top of Done/Skip), plus the two footer links to Settings and the
  // off-switch. There are no keyboard shortcuts anywhere.
  import type { DialogHabit } from "./types";

  const videoExtensionPattern = /\.(mp4|webm|ogv|mov|m4v)$/i;

  interface Props {
    /** The habit currently due, to show in full. */
    habit: DialogHabit;
    /** Called with the habit id when the user marks it done. */
    onDone: (habitId: number) => void;
    /** Called with the habit id when the user skips it. */
    onSkip: (habitId: number) => void;
    /** Called with the habit id when the user snoozes it. */
    onSnooze: (habitId: number) => void;
    /** Called when the user follows the footer Settings link. */
    onSettings: () => void;
    /** Called when the user follows the footer "Turn off nudges" link. */
    onTurnOffNudges: () => void;
  }

  let { habit, onDone, onSkip, onSnooze, onSettings, onTurnOffNudges }: Props = $props();

  let isVideo = $derived(habit.media_path !== null && videoExtensionPattern.test(habit.media_path));
  let categoryLabel = $derived(habit.category === "exercise" ? "Exercise" : "General");
</script>

<section
  class="flex w-80 flex-col gap-3 rounded-card border border-border bg-surface p-4 font-sans shadow-card"
  aria-label="{habit.name} details"
>
  {#if isVideo && habit.media_path}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video class="h-[132px] w-full rounded-lg object-cover" src={habit.media_path} controls></video>
  {:else if habit.media_path}
    <img class="h-[132px] w-full rounded-lg object-cover" src={habit.media_path} alt={habit.name} />
  {:else}
    <div
      class="flex h-[132px] w-full items-center justify-center rounded-lg bg-linear-to-br from-accent to-accent-ink text-white"
      aria-hidden="true"
    >
      <svg
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="1.4"
        stroke-linecap="round"
        stroke-linejoin="round"
        class="h-16 w-16 opacity-95"
      >
        <circle cx="12" cy="4.5" r="2" />
        <path d="M12 7v6m0 0-4 5m4-5 4 5M6 9l6 1 6-1" />
      </svg>
    </div>
  {/if}

  <span
    class="w-fit rounded-full bg-accent-soft px-2 py-0.5 font-mono text-[0.62rem] tracking-wide text-accent-ink uppercase"
  >
    {categoryLabel}
  </span>

  <h2 class="font-display text-lg font-semibold text-ink">{habit.name}</h2>

  <p class="text-sm text-ink">{habit.instructions}</p>

  {#if habit.meta}
    <p class="font-mono text-xs tracking-wide text-ink-soft uppercase" data-testid="meta-line">{habit.meta}</p>
  {/if}

  <div class="flex gap-2">
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
    <button
      class="flex-1 rounded-lg border border-border py-1.5 text-sm font-semibold text-ink-soft hover:bg-surface-2"
      type="button"
      onclick={() => onSnooze(habit.habit_id)}
    >
      Snooze
    </button>
  </div>

  <div class="flex items-center justify-between border-t border-border pt-3 text-xs">
    <button
      class="flex items-center gap-1 rounded-md px-1 py-0.5 text-ink-soft hover:bg-surface-2 hover:text-ink"
      type="button"
      onclick={onSettings}
    >
      <span aria-hidden="true">⚙</span> Settings
    </button>
    <button
      class="flex items-center gap-1 rounded-md px-1 py-0.5 text-ink-soft hover:bg-surface-2 hover:text-danger"
      type="button"
      onclick={onTurnOffNudges}
    >
      Turn off nudges
    </button>
  </div>
</section>
