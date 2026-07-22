<script lang="ts">
  // The Settings window (design spec §3.6): "Quiet rules" (calendar pause +
  // sub-choice, idle, DND) and "General" (day rollover, global day window,
  // start at login). Reads and writes the single app-wide config via the
  // get_config/set_config commands — every control persists immediately on
  // change, there's no separate Save step.
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";
  import type { CalendarMode, Config } from "./types";

  let config = $state<Config | null>(null);

  onMount(async () => {
    config = await invoke<Config>("get_config");
  });

  async function persist(next: Config): Promise<void> {
    config = next;
    await invoke("set_config", { config: next });
  }

  function toggleCalendarPause(): void {
    if (!config) return;
    void persist({ ...config, calendar_pause_enabled: !config.calendar_pause_enabled });
  }

  function setCalendarMode(mode: CalendarMode): void {
    if (!config) return;
    void persist({ ...config, calendar_mode: mode });
  }

  function toggleIdle(): void {
    if (!config) return;
    void persist({ ...config, idle_enabled: !config.idle_enabled });
  }

  function toggleDnd(): void {
    if (!config) return;
    void persist({ ...config, dnd_enabled: !config.dnd_enabled });
  }

  function toggleStartAtLogin(): void {
    if (!config) return;
    void persist({ ...config, start_at_login: !config.start_at_login });
  }

  function setDayRollover(value: string): void {
    if (!config) return;
    void persist({ ...config, day_rollover: value });
  }

  function setDayWindowStart(value: string): void {
    if (!config) return;
    void persist({ ...config, day_window_start: value });
  }

  function setDayWindowEnd(value: string): void {
    if (!config) return;
    void persist({ ...config, day_window_end: value });
  }
</script>

{#if config}
  <section
    class="mx-auto flex w-full max-w-xl flex-col gap-5 bg-ground p-5 font-sans text-ink"
    aria-label="Settings"
  >
    <div>
      <h2 class="font-display text-[1.05rem] font-semibold text-ink">Quiet rules</h2>
      <p class="mt-0.5 mb-2 text-[0.78rem] text-ink-soft">Never nudge at the wrong moment.</p>

      <div class="flex items-center justify-between gap-3 border-b border-border py-2.5">
        <div class="text-[0.84rem]">
          <label for="calendar-pause">Pause using my calendar</label>
          <p class="text-[0.72rem] text-ink-soft">
            Read the local calendar and hold nudges while you're busy
          </p>
        </div>
        <input
          id="calendar-pause"
          type="checkbox"
          class="accent-signal h-4 w-4 shrink-0 rounded"
          checked={config.calendar_pause_enabled}
          onchange={toggleCalendarPause}
        />
      </div>

      <div
        class="border-b border-border py-2.5"
        class:opacity-40={!config.calendar_pause_enabled}
        class:pointer-events-none={!config.calendar_pause_enabled}
      >
        <div
          class="inline-flex gap-0.5 rounded-[9px] border border-border bg-surface-2 p-[3px]"
          role="group"
          aria-label="Calendar pause mode"
        >
          <button
            class="rounded-md px-2.5 py-1.5 text-[0.76rem] font-semibold text-ink-soft aria-pressed:bg-surface aria-pressed:text-ink aria-pressed:shadow-soft"
            type="button"
            disabled={!config.calendar_pause_enabled}
            aria-pressed={config.calendar_mode === "with-others"}
            onclick={() => setCalendarMode("with-others")}
          >
            Events with someone else
          </button>
          <button
            class="rounded-md px-2.5 py-1.5 text-[0.76rem] font-semibold text-ink-soft aria-pressed:bg-surface aria-pressed:text-ink aria-pressed:shadow-soft"
            type="button"
            disabled={!config.calendar_pause_enabled}
            aria-pressed={config.calendar_mode === "all"}
            onclick={() => setCalendarMode("all")}
          >
            All calendar events
          </button>
        </div>
      </div>

      <div class="flex items-center justify-between gap-3 border-b border-border py-2.5">
        <div class="text-[0.84rem]">
          <label for="idle-enabled">Don't nudge when idle</label>
          <p class="text-[0.72rem] text-ink-soft">Hold and re-arm when you're back at the machine</p>
        </div>
        <input
          id="idle-enabled"
          type="checkbox"
          class="accent-accent h-4 w-4 shrink-0 rounded"
          checked={config.idle_enabled}
          onchange={toggleIdle}
        />
      </div>

      <div class="flex items-center justify-between gap-3 border-b border-border py-2.5">
        <label class="text-[0.84rem]" for="dnd-enabled">Respect Do Not Disturb / Focus</label>
        <input
          id="dnd-enabled"
          type="checkbox"
          class="accent-accent h-4 w-4 shrink-0 rounded"
          checked={config.dnd_enabled}
          onchange={toggleDnd}
        />
      </div>
    </div>

    <div>
      <h2 class="font-display text-[1.05rem] font-semibold text-ink">General</h2>
      <p class="mt-0.5 mb-2 text-[0.78rem] text-ink-soft">Housekeeping.</p>

      <div class="flex items-center justify-between gap-3 border-b border-border py-2.5">
        <label class="text-[0.84rem]" for="day-rollover">Day rollover time</label>
        <input
          id="day-rollover"
          class="w-32 rounded-lg border border-border bg-surface-2 px-2.5 py-1.5 text-sm text-ink"
          type="time"
          value={config.day_rollover}
          oninput={(event) => setDayRollover(event.currentTarget.value)}
        />
      </div>

      <div class="flex items-center justify-between gap-3 border-b border-border py-2.5">
        <label class="text-[0.84rem]" for="day-window-start">Day window start</label>
        <input
          id="day-window-start"
          class="w-32 rounded-lg border border-border bg-surface-2 px-2.5 py-1.5 text-sm text-ink"
          type="time"
          value={config.day_window_start}
          oninput={(event) => setDayWindowStart(event.currentTarget.value)}
        />
      </div>

      <div class="flex items-center justify-between gap-3 border-b border-border py-2.5">
        <label class="text-[0.84rem]" for="day-window-end">Day window end</label>
        <input
          id="day-window-end"
          class="w-32 rounded-lg border border-border bg-surface-2 px-2.5 py-1.5 text-sm text-ink"
          type="time"
          value={config.day_window_end}
          oninput={(event) => setDayWindowEnd(event.currentTarget.value)}
        />
      </div>

      <div class="flex items-center justify-between gap-3 border-b border-border py-2.5">
        <div class="text-[0.84rem]">
          <label for="start-at-login">Start at login</label>
          <p class="text-[0.72rem] text-ink-soft">Needed for the app to nudge through the day</p>
        </div>
        <input
          id="start-at-login"
          type="checkbox"
          class="accent-accent h-4 w-4 shrink-0 rounded"
          checked={config.start_at_login}
          onchange={toggleStartAtLogin}
        />
      </div>
    </div>
  </section>
{/if}
