<script lang="ts">
  import { Check, Loader, Search } from 'lucide-svelte';
  import { applyIdentify } from '$app/actions';
  import { closeIdentify, runIdentifySearch } from './actions';
  import { dropsIdentifier, pseudoDoiHint } from './input';
  import { identifyState } from './state.svelte';
  import Modal from '$shared/ui/Modal.svelte';

  // The staged identifier's value (direct never holds the candidate variant).
  const directValue = $derived(
    identifyState.direct && 'doi' in identifyState.direct
      ? identifyState.direct.doi
      : identifyState.direct && 'arxiv_id' in identifyState.direct
        ? identifyState.direct.arxiv_id
        : null,
  );
</script>

{#snippet identifyFooter()}
  {#if dropsIdentifier(identifyState)}
    <p class="mb-2 text-xs text-yellow-700 dark:text-yellow-400">
      Applying this match will drop an identifier the paper currently has
      (DOI/arXiv id not present in the selected record).
    </p>
  {/if}
  <button
    type="button"
    onclick={() => void applyIdentify()}
    disabled={identifyState.busy}
    class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 disabled:opacity-50 dark:bg-amber-600 dark:hover:bg-amber-500"
  >
    <Check size={14} /> Apply match
  </button>
{/snippet}

<Modal
  title="Identify paper"
  onclose={closeIdentify}
  footer={identifyState.selected || identifyState.direct ? identifyFooter : undefined}
>
  <form
    class="flex gap-2"
    onsubmit={(e) => {
      e.preventDefault();
      void runIdentifySearch();
    }}
  >
    <input
      bind:value={identifyState.input}
      placeholder="DOI, arXiv id, or corrected title…"
      class="min-w-0 flex-1 rounded-lg border border-stone-300 px-3 py-1.5 text-sm dark:border-stone-700 dark:bg-stone-800"
    />
    <button
      type="submit"
      disabled={identifyState.busy}
      class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 disabled:opacity-50 dark:bg-amber-600 dark:hover:bg-amber-500"
    >
      {#if identifyState.busy}<Loader size={14} class="animate-spin" />{:else}<Search size={14} />{/if}
      Search
    </button>
  </form>

  {#if identifyState.error}
    <p class="mt-3 text-sm text-red-600 dark:text-red-400">{identifyState.error}</p>
  {/if}

  {#if identifyState.direct}
    <p class="mt-3 text-sm text-stone-600 dark:text-stone-300">
      Direct identifier detected ({directValue}) — Apply fetches the authoritative record.
    </p>
    {#if pseudoDoiHint(identifyState.direct)}
      <p class="mt-2 text-xs text-yellow-700 dark:text-yellow-400">
        {pseudoDoiHint(identifyState.direct)}
      </p>
    {/if}
  {/if}

  {#if identifyState.candidates.length}
    <ul class="mt-4 space-y-1">
      {#each identifyState.candidates as c, i (i)}
        <li>
          <button
            type="button"
            onclick={() => (identifyState.selected = c)}
            class="w-full rounded-lg border px-3 py-2 text-left text-sm {identifyState.selected === c
              ? 'border-amber-600 bg-amber-700/5 dark:bg-amber-500/10'
              : 'border-stone-200 hover:bg-stone-50 dark:border-stone-700 dark:hover:bg-stone-800'}"
          >
            <span class="block font-medium">{c.title ?? '(untitled)'}</span>
            <span class="block truncate text-xs text-stone-500 dark:text-stone-400">
              {c.authors.join(', ')}
            </span>
            <span class="block text-xs text-stone-500 dark:text-stone-400">
              {c.venue ?? '?'} {c.year ?? ''}
              <span class="ml-1 rounded bg-stone-100 px-1 py-0.5 dark:bg-stone-800">{c.source}</span>
            </span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</Modal>
