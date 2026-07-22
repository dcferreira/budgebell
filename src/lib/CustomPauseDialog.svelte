<script lang="ts">
  // The Custom Pause dialog (design spec §3.4), reached from the tray's
  // "Pause nudges → Custom…" item. Lets the user pick an explicit "resume
  // at" instant via the native datetime-local picker, with three quick
  // presets that compute and set that value. There are no keyboard
  // shortcuts anywhere.

  interface Props {
    /** Called when the user cancels without pausing. */
    onCancel: () => void;
    /** Called with the chosen "resume at" instant when the user confirms. */
    onPause: (resumeAt: Date) => void;
    /** Injectable clock, defaulting to the real one — overridden in tests for deterministic presets. */
    now?: () => Date;
  }

  let { onCancel, onPause, now = () => new Date() }: Props = $props();

  let resumeAt = $state("");

  function pad(value: number): string {
    return String(value).padStart(2, "0");
  }

  // datetime-local values are local wall-clock time with no timezone suffix.
  function toDatetimeLocalValue(date: Date): string {
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
  }

  function setOneHour(): void {
    resumeAt = toDatetimeLocalValue(new Date(now().getTime() + 60 * 60 * 1000));
  }

  function setTomorrow9am(): void {
    const date = now();
    date.setDate(date.getDate() + 1);
    date.setHours(9, 0, 0, 0);
    resumeAt = toDatetimeLocalValue(date);
  }

  function setNextMonday9am(): void {
    const date = now();
    // Always the *next* Monday, never today — a Monday "now" rolls a full week forward.
    const daysUntilMonday = ((1 - date.getDay() + 7) % 7) || 7;
    date.setDate(date.getDate() + daysUntilMonday);
    date.setHours(9, 0, 0, 0);
    resumeAt = toDatetimeLocalValue(date);
  }

  function handlePause(): void {
    onPause(new Date(resumeAt));
  }
</script>

<section
  class="flex w-80 flex-col gap-3 rounded-lg border border-gray-200 bg-white p-4 shadow-lg"
  aria-label="Custom pause"
>
  <h2 class="text-lg font-semibold text-gray-900">Pause nudges</h2>

  <label class="flex flex-col gap-1 text-sm text-gray-700" for="resume-at">
    Resume at
    <input
      id="resume-at"
      class="rounded border border-gray-300 px-2 py-1"
      type="datetime-local"
      bind:value={resumeAt}
    />
  </label>

  <div class="flex gap-2">
    <button
      class="flex-1 rounded border border-gray-300 py-1.5 text-sm text-gray-700 hover:bg-gray-50"
      type="button"
      onclick={setOneHour}
    >
      1 hour
    </button>
    <button
      class="flex-1 rounded border border-gray-300 py-1.5 text-sm text-gray-700 hover:bg-gray-50"
      type="button"
      onclick={setTomorrow9am}
    >
      Tomorrow 9am
    </button>
    <button
      class="flex-1 rounded border border-gray-300 py-1.5 text-sm text-gray-700 hover:bg-gray-50"
      type="button"
      onclick={setNextMonday9am}
    >
      Next Monday 9am
    </button>
  </div>

  <div class="flex justify-end gap-2 border-t border-gray-100 pt-3">
    <button
      class="rounded border border-gray-300 px-3 py-1.5 text-sm text-gray-700 hover:bg-gray-50"
      type="button"
      onclick={onCancel}
    >
      Cancel
    </button>
    <button
      class="rounded bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-700 disabled:cursor-not-allowed disabled:opacity-50"
      type="button"
      disabled={!resumeAt}
      onclick={handlePause}
    >
      Pause
    </button>
  </div>
</section>
