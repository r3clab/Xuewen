<script lang="ts">
  import { getPaperCode, removePaperCode, setPaperCode } from './api';
  import type { PaperCodeStatus } from '$shared/api/generated/PaperCodeStatus';
  import ConfirmButtons from '$shared/ui/ConfirmButtons.svelte';
  import Spinner from '$shared/ui/Spinner.svelte';

  let { id }: { id: string } = $props();

  let code = $state<PaperCodeStatus | null>(null);
  let loaded = $state(false);
  let url = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);
  let confirmingDetach = $state(false);

  async function refresh() {
    try {
      const r = await getPaperCode(id);
      code = r.code ?? null;
      loaded = true;
    } catch (e) {
      error = (e as Error).message;
    }
  }

  // Load on mount / paper change, and poll every 2s while a clone runs.
  $effect(() => {
    void id;
    loaded = false;
    code = null;
    void refresh();
  });
  $effect(() => {
    if (code?.status !== 'cloning') return;
    const t = setInterval(() => void refresh(), 2000);
    return () => clearInterval(t);
  });

  async function attach() {
    const u = url.trim();
    if (!u) return;
    busy = true;
    error = null;
    try {
      const r = await setPaperCode(id, u);
      code = r.code;
      url = '';
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
    }
  }

  async function detach() {
    busy = true;
    error = null;
    try {
      await removePaperCode(id);
      code = null;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
      confirmingDetach = false;
    }
  }

  const label = 'text-caption font-semibold uppercase tracking-[.08em] text-stone-500 dark:text-stone-400';
</script>

<section class="mt-4 border-t border-stone-200 pt-4 dark:border-stone-800">
  <h3 class={label}>Code</h3>
  {#if !loaded && !error}
    <Spinner class="mt-2 !text-xs" />
  {:else if code === null}
    <form
      class="mt-2 flex gap-1.5"
      onsubmit={(e) => {
        e.preventDefault();
        void attach();
      }}
    >
      <input
        bind:value={url}
        placeholder="https://github.com/…"
        aria-label="Repository URL"
        class="min-w-0 flex-1 rounded-lg border border-stone-200 bg-parchment px-2 py-1 text-xs outline-none focus:border-amber-700 dark:border-stone-700 dark:bg-stone-800 dark:focus:border-amber-500"
      />
      <button
        type="submit"
        disabled={busy || !url.trim()}
        class="rounded-lg bg-amber-700 px-2.5 py-1 text-xs font-medium text-white hover:bg-amber-800 disabled:opacity-50 dark:bg-amber-600 dark:hover:bg-amber-500"
      >Attach</button>
    </form>
    <p class="mt-1.5 text-caption leading-snug text-stone-400 dark:text-stone-500">
      Attach the paper's repository so Ask can ground answers in the code.
    </p>
  {:else}
    <p class="mt-2 break-all text-xs text-stone-700 dark:text-stone-300">{code.repo_url}</p>
    <div class="mt-1.5 flex items-center gap-2">
      {#if code.status === 'cloning'}
        <span class="rounded-md bg-stone-200/70 px-1.5 py-0.5 text-chip font-medium text-stone-600 dark:bg-stone-800 dark:text-stone-300">cloning…</span>
      {:else if code.status === 'ready'}
        <span class="rounded-md bg-amber-700/10 px-1.5 py-0.5 font-mono text-chip text-amber-700 dark:bg-amber-500/15 dark:text-amber-500">ready · {code.commit_sha}</span>
      {:else}
        <span class="rounded-md bg-red-500/10 px-1.5 py-0.5 text-chip font-medium text-red-700 dark:text-red-400">{code.error ?? 'clone failed'}</span>
      {/if}
      {#if confirmingDetach}
        <ConfirmButtons
          confirmLabel="Remove repo"
          onConfirm={() => void detach()}
          onCancel={() => (confirmingDetach = false)}
        />
      {:else}
        <button
          type="button"
          onclick={() => (confirmingDetach = true)}
          disabled={busy}
          class="text-caption text-stone-500 underline-offset-2 hover:underline dark:text-stone-400"
        >Remove</button>
      {/if}
    </div>
  {/if}
  {#if error}
    <p class="mt-1.5 text-caption text-red-700 dark:text-red-400">{error}</p>
  {/if}
</section>
