<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";

  // Minimal smoke-test surface: proves the Svelte UI, Tailwind, and the
  // Rust IPC bridge are all wired up. Real habit UI replaces this later.
  let name = $state("");
  let greeting = $state("");

  async function greet(event: SubmitEvent) {
    event.preventDefault();
    greeting = await invoke("greet", { name });
  }
</script>

<main class="mx-auto flex min-h-screen max-w-md flex-col items-center justify-center gap-6 p-8">
  <h1 class="text-3xl font-bold">habits</h1>
  <p class="text-sm text-gray-500">Infrastructure smoke test</p>

  <form class="flex w-full gap-2" onsubmit={greet}>
    <input
      class="flex-1 rounded border border-gray-300 px-3 py-2"
      placeholder="Enter a name..."
      bind:value={name}
    />
    <button class="rounded bg-blue-600 px-4 py-2 font-medium text-white" type="submit">
      Greet
    </button>
  </form>

  {#if greeting}
    <p class="text-lg">{greeting}</p>
  {/if}
</main>
