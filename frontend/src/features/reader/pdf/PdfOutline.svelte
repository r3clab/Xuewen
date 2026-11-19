<script lang="ts">
  import { ChevronRight } from '@lucide/svelte';
  import { onMount, tick, untrack } from 'svelte';
  import { useOutline, useOutlineState, type OutlineItem } from '@embedpdf/svelte/outline';
  import { useStage } from '@embedpdf/svelte/stage';

  const outline = useOutline();
  const tree = useOutlineState();
  const stage = useStage();

  // A failed read shows the same quiet empty state as a PDF without one.
  const loading = $derived(tree.status === 'idle' || tree.status === 'loading');

  onMount(() => void outline.ensureLoaded().catch(() => {}));

  /// Only in-document jumps are offered: a URI, remote or launch entry is
  /// shown but inert, and none goes through the plugin's own activation.
  function pageOf(item: OutlineItem) {
    return item.target?.kind === 'goto' ? item.target.destination.page : null;
  }

  // Every section starts open, whatever the file's `/Count` says, then the
  // current one is revealed ONCE when the outline view opens (the view is
  // {#if}-gated, so mount = activation): never afterwards — auto-scrolling
  // while the user browses would fight them.
  let listEl = $state<HTMLDivElement>();
  let revealed = false;
  $effect(() => {
    if (revealed || tree.status !== 'ready') return;
    revealed = true;
    untrack(() => {
      outline.expandAll();
      const current = tree.current;
      if (!current) return;
      void tick().then(() => {
        listEl
          ?.querySelector(`[data-outline-item="${current.objectNumber}"]`)
          ?.scrollIntoView({ block: 'nearest' });
      });
    });
  });
</script>

<div bind:this={listEl} class="min-h-0 flex-1 overflow-y-auto p-1.5">
  {#if loading}
    <p class="p-2 text-xs text-stone-500 dark:text-stone-400">Loading…</p>
  {:else if tree.rows.length === 0}
    <p class="p-2 text-xs text-stone-500 dark:text-stone-400">No outline in this PDF.</p>
  {:else}
    <ul>
      {#each tree.rows as row (row.item.ref.objectNumber)}
        {@const page = pageOf(row.item)}
        <li class="flex items-center" style:padding-left="{row.depth * 0.75}rem">
          {#if row.hasChildren}
            <button
              type="button"
              aria-label={row.expanded ? 'Collapse section' : 'Expand section'}
              aria-expanded={row.expanded}
              onclick={() => outline.toggle(row.item.ref)}
              class="shrink-0 rounded p-0.5 text-stone-400 hover:text-ink dark:text-stone-500 dark:hover:text-stone-100"
            >
              <span class={`block transition-transform ${row.expanded ? 'rotate-90' : ''}`}>
                <ChevronRight size={12} />
              </span>
            </button>
          {:else}
            <span class="w-4 shrink-0"></span>
          {/if}
          <button
            type="button"
            disabled={page === null}
            onclick={() => page && stage.goToPage(page)}
            title={row.item.title}
            data-outline-item={row.item.ref.objectNumber}
            class={`min-w-0 flex-1 truncate rounded px-1 py-0.5 text-left text-xs disabled:cursor-default ${
              row.item.ref.objectNumber === tree.current?.objectNumber
                ? 'bg-amber-700/10 text-amber-700 dark:bg-amber-500/15 dark:text-amber-500'
                : 'text-stone-600 hover:bg-parchment hover:text-ink disabled:hover:bg-transparent dark:text-stone-300 dark:hover:bg-stone-800'
            }`}
          >
            {row.item.title}
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</div>
