<script lang="ts">
  // The Stats window (design spec §3.9), titled "habits — Activity". It holds
  // no logic of its own beyond rendering: everything is aggregated
  // server-side by the `day_log` command (§6.1) and merely formatted here.
  import { invoke } from "@tauri-apps/api/core";
  import { onMount, untrack } from "svelte";
  import {
    formatClockTime,
    formatDayLabel,
    formatEventDuration,
    formatGapDuration,
    formatMovingTime,
    relativeDayTag,
    shiftDateString,
    todayDateString,
  } from "./statsFormat";
  import type { DayLog, LoggedEvent } from "./types";

  interface Props {
    /** Injectable clock, defaulting to the real one — overridden in tests for a deterministic "today". */
    now?: () => Date;
  }

  let { now = () => new Date() }: Props = $props();

  // `now()` is only read once here to seed the default date — subsequent
  // navigation is driven entirely by `selectedDate`, so this deliberately
  // doesn't track further changes to the injected clock.
  let selectedDate = $state(untrack(() => todayDateString(now())));
  let dayLog = $state<DayLog | null>(null);

  let isToday = $derived(selectedDate === todayDateString(now()));

  // The single chronological activity list (design spec §3.9): `done` and
  // `skipped` events merged and sorted by time. `list_events_between` already
  // returns events ordered by `at`, so filtering preserves that order.
  let activity = $derived<LoggedEvent[]>(
    (dayLog?.events ?? []).filter((event) => event.action === "done" || event.action === "skipped"),
  );

  async function loadDay(date: string): Promise<void> {
    dayLog = await invoke<DayLog>("day_log", { date });
  }

  onMount(() => {
    void loadDay(selectedDate);
  });

  async function goToPreviousDay(): Promise<void> {
    selectedDate = shiftDateString(selectedDate, -1);
    await loadDay(selectedDate);
  }

  async function goToNextDay(): Promise<void> {
    // Forward is disabled on today (design spec §3.9) — there are no future
    // days, so guard here too against a stray click on the disabled button.
    if (isToday) return;
    selectedDate = shiftDateString(selectedDate, 1);
    await loadDay(selectedDate);
  }

  function doneDurationSecs(event: LoggedEvent): number | null {
    if (event.action !== "done" || event.shown_at === null) return null;
    return event.at - event.shown_at;
  }
</script>

<section
  class="flex w-[28rem] flex-col gap-4 rounded-lg border border-gray-200 bg-white p-4 shadow-lg"
  aria-label="habits — Activity"
>
  <header class="flex items-center justify-between">
    <button
      class="rounded px-2 py-1 text-lg text-gray-500 hover:bg-gray-100 hover:text-gray-700"
      type="button"
      aria-label="Previous day"
      onclick={goToPreviousDay}
    >
      ‹
    </button>

    <div class="text-center">
      <p class="font-semibold text-gray-900">{formatDayLabel(selectedDate)}</p>
      <p class="text-xs text-gray-500">{relativeDayTag(selectedDate, now())}</p>
    </div>

    <button
      class="rounded px-2 py-1 text-lg text-gray-500 hover:bg-gray-100 hover:text-gray-700 disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-transparent"
      type="button"
      aria-label="Next day"
      disabled={isToday}
      onclick={goToNextDay}
    >
      ›
    </button>
  </header>

  {#if dayLog}
    <div class="grid grid-cols-4 gap-2 text-center" role="group" aria-label="Day summary">
      <div data-testid="summary-done">
        <p class="text-xl font-semibold text-green-600">{dayLog.summary.done_count}</p>
        <p class="text-xs text-gray-500">Done</p>
      </div>
      <div data-testid="summary-skipped">
        <p class="text-xl font-semibold text-amber-600">{dayLog.summary.skipped_count}</p>
        <p class="text-xs text-gray-500">Skipped</p>
      </div>
      <div data-testid="summary-moving">
        <p class="text-xl font-semibold text-gray-900">{formatMovingTime(dayLog.summary.total_moving_secs)}</p>
        <p class="text-xs text-gray-500">Moving</p>
      </div>
      <div data-testid="summary-adherence">
        <p class="text-xl font-semibold text-gray-900">{Math.round(dayLog.summary.adherence_pct)}%</p>
        <p class="text-xs text-gray-500">Adherence</p>
      </div>
    </div>

    <div class="rounded border border-gray-100 p-3" data-testid="longest-sit">
      <p class="text-sm font-medium text-gray-700">Longest sit</p>
      <p class="text-lg font-semibold text-gray-900">
        {formatGapDuration(dayLog.longest_gap.duration_secs)}
        <span class="text-sm font-normal text-gray-500">
          · {formatClockTime(dayLog.longest_gap.start)}–{formatClockTime(dayLog.longest_gap.end)}
        </span>
      </p>
    </div>

    <div class="flex flex-col gap-1">
      <p class="text-xs font-medium text-gray-500">
        {activity.length} drills · {dayLog.summary.done_count} done · {dayLog.summary.skipped_count} skipped
      </p>
      <ul class="flex flex-col divide-y divide-gray-100">
        {#each activity as event (event.id)}
          <li class="flex items-center gap-3 py-2" data-testid="activity-row-{event.id}">
            {#if event.action === "done"}
              <span class="h-2.5 w-2.5 shrink-0 rounded-full bg-green-600" data-testid="status-dot-done" aria-hidden="true"></span>
            {:else}
              <span
                class="h-2.5 w-2.5 shrink-0 rounded-full border-2 border-amber-500"
                data-testid="status-ring-skipped"
                aria-hidden="true"
              ></span>
            {/if}

            <span class="w-12 shrink-0 text-xs text-gray-500">{formatClockTime(event.at)}</span>

            <span
              class="min-w-0 flex-1 truncate text-sm"
              class:text-gray-900={event.action === "done"}
              class:text-gray-400={event.action === "skipped"}
              class:line-through={event.action === "skipped"}
            >
              {event.habit_name}
            </span>

            {#if event.action === "done"}
              <span class="shrink-0 rounded-full bg-gray-100 px-2 py-0.5 text-xs text-gray-600">{event.category}</span>
              {#if doneDurationSecs(event) !== null}
                <span class="shrink-0 text-sm font-medium text-green-600">{formatEventDuration(doneDurationSecs(event) ?? 0)}</span>
              {/if}
            {:else}
              <span class="shrink-0 text-xs font-semibold tracking-wide text-amber-600">SKIPPED</span>
            {/if}
          </li>
        {/each}
      </ul>
    </div>

    <p class="text-xs text-gray-400">
      Drills you were away for aren't shown — they're withdrawn and never logged.
    </p>
  {/if}
</section>
