<script lang="ts">
  // The paused state card (design spec §3.5), shown in place of the toast
  // while nudges are paused. There are no keyboard shortcuts anywhere.

  interface Props {
    /** Called when the user clicks Resume. */
    onResume: () => void;
  }

  let { onResume }: Props = $props();
</script>

<section
  class="flex w-[250px] items-center gap-3 rounded-card border border-border bg-surface p-3.5 font-sans shadow-card"
  aria-label="Nudges paused"
>
  <span
    class="flex h-[34px] w-[34px] shrink-0 items-center justify-center rounded-lg bg-signal-soft text-signal"
    aria-hidden="true"
  >
    <svg viewBox="0 0 24 24" fill="currentColor" class="h-4 w-4">
      <rect x="6" y="5" width="4" height="14" rx="1" />
      <rect x="14" y="5" width="4" height="14" rx="1" />
    </svg>
  </span>
  <div class="min-w-0 flex-1">
    <p class="text-[0.88rem] font-semibold text-ink">Nudges paused</p>
    <p class="text-[0.74rem] text-ink-soft">No drills until you resume.</p>
  </div>
  <button
    class="shrink-0 rounded-lg bg-accent px-2.5 py-1.5 text-sm font-semibold text-white hover:brightness-105"
    type="button"
    onclick={onResume}
  >
    Resume
  </button>
</section>
