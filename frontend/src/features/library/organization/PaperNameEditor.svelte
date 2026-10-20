<script lang="ts">
  import { setPaperName } from '$app/actions';
  import { NAME_CHIP } from './nameChip';
  import type { PaperDetail } from '$shared/api/generated/PaperDetail';

  let { d }: { d: PaperDetail } = $props();

  let editing = $state(false);
  let draft = $state('');
  let busy = $state(false);
  let error = $state<string | null>(null);

  function beginEdit() {
    draft = d.name ?? '';
    error = null;
    editing = true;
  }

  // The input exists only in edit mode; focus and select it as it mounts so
  // typing replaces the old value without an extra click.
  function autofocus(el: HTMLInputElement) {
    el.focus();
    el.select();
  }

  async function commit() {
    // Disabling the focused input during the request fires a browser blur,
    // which re-enters here while the first call is still in flight — the busy
    // guard (checked against plain $state, not the DOM) makes that a no-op.
    if (busy || !editing) return;
    const trimmed = draft.trim();
    const next = trimmed === '' ? null : trimmed;
    if (next === (d.name ?? null)) {
      editing = false;
      error = null;
      return;
    }
    busy = true;
    error = null;
    try {
      // On success the patched detail cache re-renders this row with the
      // server-confirmed value; on failure stay in edit mode with the
      // draft intact so the user can fix and retry.
      await setPaperName(d.id, next);
      editing = false;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault();
      void commit();
    } else if (e.key === 'Escape') {
      editing = false;
      error = null;
    }
  }
</script>

<dt class="text-stone-500 dark:text-stone-400">Name</dt>
<dd>
  {#if editing}
    <input
      bind:value={draft}
      use:autofocus
      disabled={busy}
      onblur={() => void commit()}
      onkeydown={onKeydown}
      type="text"
      maxlength="200"
      aria-label="Paper name"
      placeholder="e.g. RVSpec"
      class="w-full rounded border border-stone-200 bg-parchment px-1.5 py-0.5 font-mono text-caption text-ink outline-none focus:border-amber-700 disabled:opacity-50 dark:border-stone-700 dark:bg-stone-800 dark:text-stone-200 dark:focus:border-amber-500"
    />
    {#if error}
      <p class="mt-0.5 text-xs text-red-600 dark:text-red-400">{error}</p>
    {/if}
  {:else}
    <!-- Set: the amber mono chip, so it stops being indistinguishable from
         the Cite key row right below, which is the *other* mono identifier
         in this list. Unset: the quiet dotted-underline prompt, unchanged. -->
    <button
      type="button"
      aria-label="Edit paper name"
      onclick={beginEdit}
      class={`text-left focus-visible:outline focus-visible:outline-2 focus-visible:outline-amber-700 dark:focus-visible:outline-amber-500 ${
        d.name
          ? `${NAME_CHIP} hover:bg-amber-700/20 dark:hover:bg-amber-500/25`
          : 'rounded text-caption text-stone-400 underline-offset-2 decoration-dotted hover:underline dark:text-stone-500'
      }`}
    >
      {d.name ?? 'Add name…'}
    </button>
  {/if}
</dd>
