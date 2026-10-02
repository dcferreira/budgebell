<script lang="ts">
  // The Settings window's "Updates" section: the running version, the result
  // of a check against the latest GitHub release (via the Tauri updater
  // plugin's signed `latest.json`), and a button that installs it and
  // relaunches. Checks once on open; the tray's "Check for updates…" item
  // opens Settings and emits `check-for-updates` so an already-open window
  // re-checks too.
  import { getVersion } from "@tauri-apps/api/app";
  import { listen } from "@tauri-apps/api/event";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { check, type Update } from "@tauri-apps/plugin-updater";
  import { onDestroy, onMount } from "svelte";

  type Status =
    | { kind: "checking" }
    | { kind: "current" }
    | { kind: "available"; update: Update }
    | { kind: "installing"; update: Update }
    | { kind: "error"; message: string };

  let version = $state<string | null>(null);
  let status = $state<Status>({ kind: "checking" });
  let unlisten: (() => void) | null = null;
  let destroyed = false;
  // Bumped per check, so a slow earlier check can't overwrite a newer result.
  let latestCheck = 0;

  function messageOf(error: unknown): string {
    return error instanceof Error ? error.message : String(error);
  }

  async function runCheck(): Promise<void> {
    // A check mid-install would re-enable Install and allow a second,
    // concurrent install over the same file.
    if (status.kind === "installing") return;
    const thisCheck = ++latestCheck;
    status = { kind: "checking" };
    let next: Status;
    try {
      const update = await check();
      next = update ? { kind: "available", update } : { kind: "current" };
    } catch (error) {
      next = { kind: "error", message: `Couldn't check for updates: ${messageOf(error)}` };
    }
    if (thisCheck === latestCheck) status = next;
  }

  async function install(update: Update): Promise<void> {
    status = { kind: "installing", update };
    try {
      await update.downloadAndInstall();
      await relaunch();
    } catch (error) {
      status = { kind: "error", message: `Couldn't install the update: ${messageOf(error)}` };
    }
  }

  onMount(() => {
    void runCheck();
    getVersion()
      .then((v) => (version = v))
      .catch(() => (version = "unknown"));
    void listen("check-for-updates", () => void runCheck()).then((stop) => {
      // Unmounted before the listener was registered: drop it at once.
      if (destroyed) stop();
      else unlisten = stop;
    });
  });

  onDestroy(() => {
    destroyed = true;
    unlisten?.();
  });
</script>

<div>
  <h2 class="font-display text-[1.05rem] font-semibold text-ink">Updates</h2>
  <p class="mt-0.5 mb-2 text-[0.78rem] text-ink-soft">New releases from GitHub.</p>

  <div class="flex items-center justify-between gap-3 border-b border-border py-2.5">
    <span class="text-[0.84rem]">Current version</span>
    <span class="text-[0.84rem] font-semibold tabular-nums">{version ?? "…"}</span>
  </div>

  <div class="flex items-center justify-between gap-3 border-b border-border py-2.5">
    <p class="text-[0.84rem]" role="status">
      {#if status.kind === "checking"}
        Checking for updates…
      {:else if status.kind === "current"}
        You're up to date.
      {:else if status.kind === "available"}
        Version {status.update.version} is available.
      {:else if status.kind === "installing"}
        Installing {status.update.version}…
      {:else}
        {status.message}
      {/if}
    </p>
    {#if status.kind === "available" || status.kind === "installing"}
      <button
        class="shrink-0 rounded-lg bg-signal px-3 py-1.5 text-[0.78rem] font-semibold text-white disabled:opacity-50"
        type="button"
        disabled={status.kind === "installing"}
        onclick={() => status.kind === "available" && void install(status.update)}
      >
        Install and restart
      </button>
    {:else}
      <button
        class="shrink-0 rounded-lg border border-border bg-surface-2 px-3 py-1.5 text-[0.78rem] font-semibold text-ink disabled:opacity-50"
        type="button"
        disabled={status.kind === "checking"}
        onclick={() => void runCheck()}
      >
        Check now
      </button>
    {/if}
  </div>
</div>
