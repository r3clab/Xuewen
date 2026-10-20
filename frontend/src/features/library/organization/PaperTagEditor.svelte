<script lang="ts">
  import { X } from 'lucide-svelte';
  import { addTagToPaper, removeTagFromPaper } from './actions';
  import { tags } from './state.svelte';
  import type { PaperDetail } from '$shared/api/generated/PaperDetail';

  let { d }: { d: PaperDetail } = $props();

  let query = $state('');
  let highlighted = $state(-1);
  let error = $state<string | null>(null);

  // Substring match against the global tags store, excluding tags already on
  // this paper — no point suggesting a re-add.
  const suggestions = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return tags.items
      .filter((t) => !d.tags.some((dt) => dt.id === t.id))
      .filter((t) => t.name.toLowerCase().includes(q))
      .slice(0, 8);
  });

  function onInput() {
    highlighted = -1;
  }

  async function commit(name: string) {
    const trimmed = name.trim();
    if (!trimmed) return;
    query = '';
    highlighted = -1;
    error = null;
    try {
      // Creates the tag if it's new and attaches it either way.
      await addTagToPaper(d.id, trimmed);
    } catch (e) {
      error = (e as Error).message;
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (suggestions.length) highlighted = (highlighted + 1) % suggestions.length;
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (suggestions.length) highlighted = (highlighted - 1 + suggestions.length) % suggestions.length;
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const pick = highlighted >= 0 ? suggestions[highlighted]?.name : undefined;
      void commit(pick ?? query);
    } else if (e.key === 'Escape') {
      query = '';
      highlighted = -1;
    }
  }

  async function onRemove(tagId: string) {
    error = null;
    try {
      await removeTagFromPaper(d.id, tagId);
    } catch (e) {
      error = (e as Error).message;
    }
  }
</script>

<h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-stone-500 dark:text-stone-400">Tags</h3>
{#if d.tags.length}
  <div class="flex flex-wrap gap-1.5">
    {#each d.tags as tag (tag.id)}
      <span
        class="inline-flex items-center gap-1 rounded border border-amber-700/40 bg-amber-700/10 px-1.5 py-0.5 text-xs font-medium text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-400"
      >
        {tag.name}
        <button
          type="button"
          aria-label={`Remove tag ${tag.name}`}
          onclick={() => void onRemove(tag.id)}
          class="rounded-full hover:bg-amber-700/20 dark:hover:bg-amber-500/20"
        >
          <X size={11} />
        </button>
      </span>
    {/each}
  </div>
{/if}
<div class="relative mt-2">
  <input
    bind:value={query}
    oninput={onInput}
    onkeydown={onKeydown}
    type="text"
    aria-label="Add a tag"
    placeholder="Add a tag…"
    class="w-full rounded-lg border border-stone-200 bg-parchment px-2 py-1 text-xs dark:border-stone-700 dark:bg-stone-800"
  />
  {#if suggestions.length}
    <ul
      class="absolute z-10 mt-1 max-h-40 w-full overflow-y-auto rounded-lg border border-stone-200 bg-paper py-1 text-xs shadow-lg dark:border-stone-700 dark:bg-soot"
    >
      {#each suggestions as s, i (s.id)}
        <li>
          <button
            type="button"
            onclick={() => void commit(s.name)}
            class={`block w-full px-2 py-1 text-left ${
              i === highlighted
                ? 'bg-amber-700/10 text-amber-800 dark:bg-amber-500/10 dark:text-amber-400'
                : 'hover:bg-parchment dark:hover:bg-stone-800'
            }`}
          >
            {s.name}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>
{#if error}
  <p class="mt-1 text-xs text-red-600 dark:text-red-400">{error}</p>
{/if}
