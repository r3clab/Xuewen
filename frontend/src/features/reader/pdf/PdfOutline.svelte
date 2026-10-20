<script lang="ts">
  import { ChevronRight } from 'lucide-svelte';
  import { tick } from 'svelte';
  import { useBookmarkCapability } from '@embedpdf/plugin-bookmark/svelte';
  import { useScroll } from '@embedpdf/plugin-scroll/svelte';
  import { currentOutlinePath, toOutline, type OutlineNode } from './outline';

  let { documentId }: { documentId: string } = $props();
  const bookmarks = useBookmarkCapability();
  const scroll = useScroll(() => documentId);

  // null = loading; [] = none (or the fetch failed — same quiet empty state).
  let nodes = $state<OutlineNode[] | null>(null);
  let requested = false;
  $effect(() => {
    const cap = bookmarks.provides;
    if (!cap || requested) return;
    requested = true;
    cap.forDocument(documentId).getBookmarks().wait(
      ({ bookmarks: list }) => (nodes = toOutline(list)),
      () => (nodes = []),
    );
  });

  // Collapsed nodes, keyed by index path ("0.2.1") — titles can repeat.
  let collapsed = $state<Set<string>>(new Set());
  function toggleNode(path: string): void {
    const next = new Set(collapsed);
    if (next.has(path)) next.delete(path);
    else next.add(path);
    collapsed = next;
  }

  function jump(n: OutlineNode): void {
    if (n.pageIndex === null) return;
    scroll.provides?.scrollToPage({ pageNumber: n.pageIndex + 1 });
  }

  const currentPath = $derived(nodes ? currentOutlinePath(nodes, scroll.state.currentPage - 1) : null);

  // Reveal the current section ONCE when the outline view opens (the view
  // is {#if}-gated, so mount = activation): expand its ancestors, then
  // scroll its row into view. Never afterwards — auto-scrolling while the
  // user browses would fight them, the same failure the thumbnail pane had.
  let listEl = $state<HTMLDivElement>();
  let revealed = false;
  $effect(() => {
    if (revealed || nodes === null) return;
    revealed = true;
    const path = currentPath;
    if (!path) return;
    const parts = path.split('.');
    const ancestors = parts.slice(0, -1).map((_, i) => parts.slice(0, i + 1).join('.'));
    if (ancestors.some((a) => collapsed.has(a))) {
      const next = new Set(collapsed);
      for (const a of ancestors) next.delete(a);
      collapsed = next;
    }
    void tick().then(() => {
      listEl?.querySelector(`[data-outline-path="${path}"]`)?.scrollIntoView({ block: 'nearest' });
    });
  });
</script>

{#snippet row(n: OutlineNode, path: string)}
  <li>
    <div class="flex items-center" style:padding-left="{n.depth * 0.75}rem">
      {#if n.children.length > 0}
        <button
          type="button"
          aria-label={collapsed.has(path) ? 'Expand section' : 'Collapse section'}
          aria-expanded={!collapsed.has(path)}
          onclick={() => toggleNode(path)}
          class="shrink-0 rounded p-0.5 text-stone-400 hover:text-ink dark:text-stone-500 dark:hover:text-stone-100"
        >
          <span class={`block transition-transform ${collapsed.has(path) ? '' : 'rotate-90'}`}>
            <ChevronRight size={12} />
          </span>
        </button>
      {:else}
        <span class="w-4 shrink-0"></span>
      {/if}
      <button
        type="button"
        disabled={n.pageIndex === null}
        onclick={() => jump(n)}
        title={n.title}
        data-outline-path={path}
        class={`min-w-0 flex-1 truncate rounded px-1 py-0.5 text-left text-xs disabled:cursor-default ${
          path === currentPath
            ? 'bg-amber-700/10 text-amber-700 dark:bg-amber-500/15 dark:text-amber-500'
            : 'text-stone-600 hover:bg-parchment hover:text-ink disabled:hover:bg-transparent dark:text-stone-300 dark:hover:bg-stone-800'
        }`}
      >
        {n.title}
      </button>
    </div>
    {#if n.children.length > 0 && !collapsed.has(path)}
      <ul>
        {#each n.children as c, i (i)}
          {@render row(c, `${path}.${i}`)}
        {/each}
      </ul>
    {/if}
  </li>
{/snippet}

<div bind:this={listEl} class="min-h-0 flex-1 overflow-y-auto p-1.5">
  {#if nodes === null}
    <p class="p-2 text-xs text-stone-500 dark:text-stone-400">Loading…</p>
  {:else if nodes.length === 0}
    <p class="p-2 text-xs text-stone-500 dark:text-stone-400">No outline in this PDF.</p>
  {:else}
    <ul>
      {#each nodes as n, i (i)}
        {@render row(n, String(i))}
      {/each}
    </ul>
  {/if}
</div>
