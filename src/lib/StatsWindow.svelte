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
  import type { DayLog, LoggedEvent, Meeting } from "./types";

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

  // The day's calendar events (design spec §3.9) shown as context rows: they
  // are already filtered server-side to mirror the meeting-pause rule, so the
  // window just interleaves them with the movements by start time. Meetings
  // never touch the summary tiles or the longest-sit — they are context only.
  type ActivityRow =
    | { kind: "event"; key: string; testid: string; at: number; event: LoggedEvent }
    | { kind: "meeting"; key: string; testid: string; at: number; meeting: Meeting };

  let rows = $derived<ActivityRow[]>(
    [
      ...activity.map(
        (event): ActivityRow => ({
          kind: "event",
          key: `event-${event.id}`,
          testid: `activity-row-${event.id}`,
          at: event.at,
          event,
        }),
      ),
      ...(dayLog?.meetings ?? []).map(
        (meeting, index): ActivityRow => ({
          kind: "meeting",
          key: `meeting-${index}`,
          testid: `meeting-row-${index}`,
          at: meeting.start,
          meeting,
        }),
      ),
    ].sort((a, b) => a.at - b.at),
  );

  /** A subtle head-count hint for a meeting row — total people, including the
   * user, shown only for a call (an event with at least one other attendee). */
  function attendeeHint(meeting: Meeting): string | null {
    if (!meeting.is_call) return null;
    const people = meeting.attendee_count + 1;
    return `${people} people`;
  }

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
  class="mx-auto flex w-full max-w-xl flex-col gap-4 bg-ground p-4 font-sans text-ink"
  aria-label="habits — Activity"
>
  <header class="flex items-center justify-center gap-3">
    <button
      class="flex h-8 w-8 items-center justify-center rounded-lg border border-border bg-surface text-ink hover:bg-surface-2"
      type="button"
      aria-label="Previous day"
      onclick={goToPreviousDay}
    >
      ‹
    </button>

    <div class="min-w-[210px] text-center">
      <p class="font-display text-[1.15rem] font-semibold text-ink">{formatDayLabel(selectedDate)}</p>
      <p class="mt-0.5 font-mono text-[0.68rem] tracking-wide text-ink-soft uppercase">
        {relativeDayTag(selectedDate, now())}
      </p>
    </div>

    <button
      class="flex h-8 w-8 items-center justify-center rounded-lg border border-border bg-surface text-ink hover:bg-surface-2 disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-surface"
      type="button"
      aria-label="Next day"
      disabled={isToday}
      onclick={goToNextDay}
    >
      ›
    </button>
  </header>

  {#if dayLog}
    <div class="grid grid-cols-4 gap-2.5" role="group" aria-label="Day summary">
      <div class="rounded-[11px] border border-border bg-surface p-3" data-testid="summary-done">
        <p class="font-mono text-[0.6rem] tracking-wide text-ink-soft uppercase">Done</p>
        <p class="mt-0.5 text-2xl leading-none font-bold tabular-nums text-good">{dayLog.summary.done_count}</p>
      </div>
      <div class="rounded-[11px] border border-border bg-surface p-3" data-testid="summary-skipped">
        <p class="font-mono text-[0.6rem] tracking-wide text-ink-soft uppercase">Skipped</p>
        <p class="mt-0.5 text-2xl leading-none font-bold tabular-nums text-signal">
          {dayLog.summary.skipped_count}
        </p>
      </div>
      <div class="rounded-[11px] border border-border bg-surface p-3" data-testid="summary-moving">
        <p class="font-mono text-[0.6rem] tracking-wide text-ink-soft uppercase">Moving</p>
        <p class="mt-0.5 text-2xl leading-none font-bold tabular-nums text-ink">
          {formatMovingTime(dayLog.summary.total_moving_secs)}
        </p>
      </div>
      <div class="rounded-[11px] border border-border bg-surface p-3" data-testid="summary-adherence">
        <p class="font-mono text-[0.6rem] tracking-wide text-ink-soft uppercase">Adherence</p>
        <p class="mt-0.5 text-2xl leading-none font-bold tabular-nums text-ink">
          {Math.round(dayLog.summary.adherence_pct)}%
        </p>
      </div>
    </div>

    <div
      class="flex items-center gap-3 rounded-[11px] border border-l-[3px] border-border border-l-signal bg-[color-mix(in_srgb,var(--color-signal)_8%,var(--color-surface))] p-3"
      data-testid="longest-sit"
    >
      <span class="flex h-[34px] w-[34px] shrink-0 items-center justify-center rounded-lg bg-signal-soft text-signal">
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="h-[18px] w-[18px]">
          <circle cx="12" cy="12" r="9" />
          <path d="M12 7v5l3 2" />
        </svg>
      </span>
      <div>
        <p class="font-mono text-[0.6rem] tracking-wide text-ink-soft uppercase">Longest sit</p>
        {#if dayLog.longest_gap}
          <p class="text-[1.1rem] font-bold tabular-nums text-ink">
            {formatGapDuration(dayLog.longest_gap.duration_secs)}
            <span class="ml-1.5 text-[0.82rem] font-normal text-ink-soft">
              · {formatClockTime(dayLog.longest_gap.start)}–{formatClockTime(dayLog.longest_gap.end)}
            </span>
          </p>
        {:else}
          <p class="text-[1.1rem] font-bold text-ink-soft">Not enough movements yet</p>
        {/if}
      </div>
    </div>

    <div class="flex flex-col">
      <div class="mb-1.5 flex items-baseline justify-between">
        <h3 class="font-display text-[1.05rem] font-semibold text-ink">Activity</h3>
        <span class="font-mono text-[0.72rem] text-ink-soft">
          {activity.length} drills · {dayLog.summary.done_count} done · {dayLog.summary.skipped_count} skipped
        </span>
      </div>
      <ul class="flex flex-col">
        {#each rows as row (row.key)}
          {#if row.kind === "event"}
            {@const event = row.event}
            <li
              class="grid grid-cols-[14px_52px_1fr_auto] items-center gap-2.5 border-b border-border py-1.5 last:border-none"
              data-testid={row.testid}
            >
              {#if event.action === "done"}
                <span class="h-2.5 w-2.5 shrink-0 rounded-full bg-good" data-testid="status-dot-done" aria-hidden="true"></span>
              {:else}
                <span
                  class="h-2.5 w-2.5 shrink-0 rounded-full border-[1.5px] border-signal"
                  data-testid="status-ring-skipped"
                  aria-hidden="true"
                ></span>
              {/if}

              <span class="font-mono text-[0.76rem] tabular-nums text-ink-soft">{formatClockTime(event.at)}</span>

              <span
                class="min-w-0 truncate text-[0.88rem]"
                class:text-ink={event.action === "done"}
                class:text-ink-soft={event.action === "skipped"}
                class:line-through={event.action === "skipped"}
              >
                {event.habit_name}
              </span>

              {#if event.action === "done"}
                <span class="flex shrink-0 items-center gap-1.5 justify-self-end">
                  <span class="rounded-full bg-accent-soft px-1.5 py-0.5 font-mono text-[0.56rem] tracking-wide text-accent-ink uppercase"
                    >{event.category}</span
                  >
                  {#if doneDurationSecs(event) !== null}
                    <span class="font-mono text-[0.78rem] font-semibold tabular-nums text-good">
                      {formatEventDuration(doneDurationSecs(event) ?? 0)}
                    </span>
                  {/if}
                </span>
              {:else}
                <span class="justify-self-end font-mono text-[0.68rem] font-semibold tracking-wide text-signal uppercase">
                  SKIPPED
                </span>
              {/if}
            </li>
          {:else}
            {@const meeting = row.meeting}
            <!-- A calendar event — neutral context row, distinct from the
                 green done-dot and amber skip-ring: a people glyph, the title,
                 a subtle head-count, and the time range in the trailing slot. -->
            <li
              class="grid grid-cols-[14px_52px_1fr_auto] items-center gap-2.5 border-b border-border py-1.5 last:border-none"
              data-testid={row.testid}
            >
              <span class="shrink-0 text-ink-soft" data-testid="meeting-glyph" aria-hidden="true">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="h-3.5 w-3.5">
                  <path d="M17 21v-2a4 4 0 0 0-4-4H5a4 4 0 0 0-4 4v2" />
                  <circle cx="9" cy="7" r="4" />
                  <path d="M23 21v-2a4 4 0 0 0-3-3.87" />
                  <path d="M16 3.13a4 4 0 0 1 0 7.75" />
                </svg>
              </span>

              <span class="font-mono text-[0.76rem] tabular-nums text-ink-soft">{formatClockTime(meeting.start)}</span>

              <span class="flex min-w-0 items-center gap-2 text-[0.88rem] text-ink-soft">
                <span class="truncate">{meeting.title}</span>
                {#if attendeeHint(meeting)}
                  <span
                    class="shrink-0 rounded-full bg-surface-2 px-1.5 py-0.5 font-mono text-[0.56rem] tracking-wide text-ink-soft uppercase"
                  >
                    {attendeeHint(meeting)}
                  </span>
                {/if}
              </span>

              <span class="justify-self-end font-mono text-[0.76rem] tabular-nums text-ink-soft">
                {formatClockTime(meeting.start)}–{formatClockTime(meeting.end)}
              </span>
            </li>
          {/if}
        {/each}
      </ul>
    </div>

    <p class="border-t border-dashed border-border pt-2.5 text-[0.74rem] text-ink-soft">
      Drills you were away for aren't shown — they're withdrawn and never logged.
    </p>
  {/if}
</section>
