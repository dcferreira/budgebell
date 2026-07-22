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
  class="flex w-96 flex-col gap-3 rounded-lg border border-gray-200 bg-white p-4 shadow-lg"
  aria-label="{habit.name} details"
>
  {#if isVideo && habit.media_path}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video class="h-48 w-full rounded object-cover" src={habit.media_path} controls></video>
  {:else if habit.media_path}
    <img class="h-48 w-full rounded object-cover" src={habit.media_path} alt={habit.name} />
  {:else}
    <div class="flex h-48 w-full items-center justify-center rounded bg-gray-100" aria-hidden="true"></div>
  {/if}

  <span
    class="w-fit rounded-full bg-blue-50 px-2 py-0.5 text-xs font-medium text-blue-700"
  >
    {categoryLabel}
  </span>

  <h2 class="text-lg font-semibold text-gray-900">{habit.name}</h2>

  <p class="text-sm text-gray-700">{habit.instructions}</p>

  {#if habit.meta}
    <p class="text-sm text-gray-500" data-testid="meta-line">{habit.meta}</p>
  {/if}

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
    <button
      class="flex-1 rounded border border-gray-300 py-1.5 text-sm font-medium text-gray-700 hover:bg-gray-50"
      type="button"
      onclick={() => onSnooze(habit.habit_id)}
    >
      Snooze
    </button>
  </div>

  <div class="flex justify-between border-t border-gray-100 pt-2 text-xs">
    <button class="text-gray-500 hover:text-gray-700" type="button" onclick={onSettings}>
      <span aria-hidden="true">⚙</span> Settings
    </button>
    <button class="text-red-600 hover:text-red-700" type="button" onclick={onTurnOffNudges}>
      Turn off nudges
    </button>
  </div>
</section>
