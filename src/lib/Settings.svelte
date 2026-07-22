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
  <section class="flex w-96 flex-col gap-6 rounded-lg border border-gray-200 bg-white p-4 shadow-lg" aria-label="Settings">
    <div class="flex flex-col gap-2">
      <h2 class="text-lg font-semibold text-gray-900">Quiet rules</h2>

      <label class="flex items-center gap-2 text-sm text-gray-700" for="calendar-pause">
        <input
          id="calendar-pause"
          type="checkbox"
          checked={config.calendar_pause_enabled}
          onchange={toggleCalendarPause}
        />
        Pause using my calendar
      </label>

      <div
        class="ml-6 flex gap-2"
        class:opacity-50={!config.calendar_pause_enabled}
        role="group"
        aria-label="Calendar pause mode"
      >
        <button
          class="flex-1 rounded border border-gray-300 py-1 text-sm text-gray-700 aria-pressed:border-blue-600 aria-pressed:bg-blue-50 aria-pressed:text-blue-700"
          type="button"
          disabled={!config.calendar_pause_enabled}
          aria-pressed={config.calendar_mode === "with-others"}
          onclick={() => setCalendarMode("with-others")}
        >
          Events with someone else
        </button>
        <button
          class="flex-1 rounded border border-gray-300 py-1 text-sm text-gray-700 aria-pressed:border-blue-600 aria-pressed:bg-blue-50 aria-pressed:text-blue-700"
          type="button"
          disabled={!config.calendar_pause_enabled}
          aria-pressed={config.calendar_mode === "all"}
          onclick={() => setCalendarMode("all")}
        >
          All calendar events
        </button>
      </div>

      <label class="flex items-center gap-2 text-sm text-gray-700" for="idle-enabled">
        <input id="idle-enabled" type="checkbox" checked={config.idle_enabled} onchange={toggleIdle} />
        Don't nudge when idle
      </label>

      <label class="flex items-center gap-2 text-sm text-gray-700" for="dnd-enabled">
        <input id="dnd-enabled" type="checkbox" checked={config.dnd_enabled} onchange={toggleDnd} />
        Respect Do Not Disturb / Focus
      </label>
    </div>

    <div class="flex flex-col gap-2">
      <h2 class="text-lg font-semibold text-gray-900">General</h2>

      <label class="flex flex-col gap-1 text-sm text-gray-700" for="day-rollover">
        Day rollover time
        <input
          id="day-rollover"
          class="w-32 rounded border border-gray-300 px-2 py-1"
          type="time"
          value={config.day_rollover}
          oninput={(event) => setDayRollover(event.currentTarget.value)}
        />
      </label>

      <div class="flex gap-3">
        <label class="flex flex-col gap-1 text-sm text-gray-700" for="day-window-start">
          Day window start
          <input
            id="day-window-start"
            class="w-32 rounded border border-gray-300 px-2 py-1"
            type="time"
            value={config.day_window_start}
            oninput={(event) => setDayWindowStart(event.currentTarget.value)}
          />
        </label>
        <label class="flex flex-col gap-1 text-sm text-gray-700" for="day-window-end">
          Day window end
          <input
            id="day-window-end"
            class="w-32 rounded border border-gray-300 px-2 py-1"
            type="time"
            value={config.day_window_end}
            oninput={(event) => setDayWindowEnd(event.currentTarget.value)}
          />
        </label>
      </div>

      <label class="flex items-center gap-2 text-sm text-gray-700" for="start-at-login">
        <input
          id="start-at-login"
          type="checkbox"
          checked={config.start_at_login}
          onchange={toggleStartAtLogin}
        />
        Start at login
      </label>
    </div>
  </section>
{/if}
