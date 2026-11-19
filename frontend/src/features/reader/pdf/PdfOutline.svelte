<script lang="ts">
  import { ChevronRight } from '@lucide/svelte';
  import { flushSync, onMount } from 'svelte';
  import { useOutline, useOutlineState, type OutlineItem } from '@embedpdf/svelte/outline';
  import { useStage } from '@embedpdf/svelte/stage';

  const outline = useOutline();
  const tree = useOutlineState();
  const stage = useStage();

  // A failed read shows the same quiet empty state as a PDF without one.
  const loading = $derived(tree.status === 'idle' || tree.status === 'loading');

  /// Only in-document jumps are offered: a URI, remote or launch entry is
  /// shown but inert, and none goes through the plugin's own activation.
  function destinationOf(item: OutlineItem) {
    return item.target?.kind === 'goto' ? item.target.destination : null;
  }

  // The first read folds the outline to its top level, whatever the file's
  // `/Count` says; after that what is expanded is the reader's, kept per
  // document by the plugin. Each time the view opens (it is {#if}-gated, so
  // mount = activation) the current section is revealed once, never
  // afterwards: auto-scrolling while the reader browses would fight them.
  let listEl: HTMLDivElement;
  onMount(() => {
    const firstRead = tree.status === 'idle';
    const read = new AbortController();
    outline.ensureLoaded({ signal: read.signal }).then(
      () => {
        if (firstRead) outline.collapseAll();
        const current = tree.current;
        if (!current) return;
        outline.reveal(current);
        flushSync();
        listEl.querySelector('[aria-current="location"]')?.scrollIntoView({ block: 'nearest' });
      },
      () => {},
    );
    return () => read.abort();
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
        {@const destination = destinationOf(row.item)}
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
            disabled={destination === null}
            onclick={() => destination && stage.goToDestination(destination)}
            title={row.item.title}
            aria-current={row.item.ref.objectNumber === tree.current?.objectNumber ? 'location' : undefined}
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
