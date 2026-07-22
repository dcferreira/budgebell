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
  class="flex w-72 items-center justify-between rounded-lg border border-gray-200 bg-white p-3 shadow-lg"
  aria-label="Nudges paused"
>
  <p class="text-sm font-medium text-gray-700">Nudges paused</p>
  <button
    class="rounded bg-blue-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-blue-700"
    type="button"
    onclick={onResume}
  >
    Resume
  </button>
</section>
