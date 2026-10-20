<script lang="ts">
  import { untrack } from 'svelte';
  import { ThumbImg, ThumbnailsPane, useThumbnailPlugin } from '@embedpdf/plugin-thumbnail/svelte';
  import type { ThumbnailDocumentState } from '@embedpdf/plugin-thumbnail';
  import { useScroll } from '@embedpdf/plugin-scroll/svelte';
  import { Highlighter, LayoutGrid, List } from 'lucide-svelte';
  import { panelWidth, reader, setPanelView } from '$features/reader/state.svelte';
  import PdfOutline from './PdfOutline.svelte';
  import PdfAnnotations from '$features/reader/annotations/PdfAnnotations.svelte';

  let { documentId }: { documentId: string } = $props();
  const scroll = useScroll(() => documentId);
  const thumbs = useThumbnailPlugin();
  let thumbsWrap = $state<HTMLDivElement>();

  // getDocumentState is declared `private` in the plugin's .d.ts, but it is
  // a public runtime method verified against @embedpdf/plugin-thumbnail
  // 2.14.4 dist (`getDocumentState(documentId) { return
  // this.state.documents[id] ?? null; }`). Minimal type-only accommodation
  // to call it; no behavior change.
  type ThumbnailPluginState = {
    getDocumentState(id: string): ThumbnailDocumentState | null;
  };
  // While the close animation runs, reader.panel is already null but the
  // panel is still visible — keep showing the last-used view as it slides
  // away instead of blanking.
  const tab = $derived(reader.panel ?? reader.lastPanel);

  function jump(pageIndex: number): void {
    scroll.provides?.scrollToPage({ pageNumber: pageIndex + 1 });
  }

  const seg = (active: boolean) =>
    `flex flex-1 items-center justify-center rounded-md px-2 py-1 ${
      active
        ? 'bg-parchment text-ink dark:bg-stone-800 dark:text-stone-100'
        : 'text-stone-500 hover:text-ink dark:text-stone-400 dark:hover:text-stone-100'
    }`;

  $effect(() => {
    if (tab !== 'thumbs') return;
    // Position the pane at the current page ONCE per thumbs-view activation
    // by setting scrollTop directly. Do NOT use scrollToThumb: the plugin's
    // scrollTo$ emitter caches its last emission (cache defaults to true in
    // @embedpdf 2.14.4) and ThumbnailsPane re-subscribes onScrollTo whenever
    // its window changes — i.e. on every manual pane scroll — replaying the
    // cached position and yanking the pane back. Direct scrollTop keeps that
    // cache empty forever. untrack: nothing below may register effect deps
    // (tracked dep = `tab` alone); plugin state and currentPage are read
    // inside rAF callbacks only.
    return untrack(() => {
      let tries = 0;
      let raf = requestAnimationFrame(function attempt() {
        const pane = thumbsWrap?.firstElementChild;
        // typeof-guarded: getDocumentState is a private-in-.d.ts runtime
        // method (see ThumbnailPluginState above). If a future plugin
        // version renames/removes it, this degrades into the retry/give-up
        // path below instead of throwing.
        const plugin = thumbs.plugin as ThumbnailPluginState | null;
        const item =
          typeof plugin?.getDocumentState === 'function'
            ? plugin.getDocumentState(documentId)?.thumbs[scroll.state.currentPage - 1]
            : undefined;
        if (pane instanceof HTMLElement && item) {
          // 8 = upstream scrollToThumb's own `margin` constant
          // (@embedpdf/plugin-thumbnail 2.14.4 dist/index.js:352),
          // replicated so direct positioning matches the plugin's semantics.
          pane.scrollTop = Math.max(0, item.top - 8);
          return; // positioned — stop retrying
        }
        if (++tries < 30) raf = requestAnimationFrame(attempt);
      });
      return () => cancelAnimationFrame(raf);
    });
  });
</script>

<div
  class="flex shrink-0 flex-col border-r border-stone-200 bg-paper dark:border-stone-800 dark:bg-night"
  style={`width:${panelWidth(tab)}px`}
>
  <div class="border-b border-stone-200 p-1.5 dark:border-stone-800">
    <div class="flex gap-0.5 rounded-lg bg-stone-100 p-0.5 dark:bg-stone-900">
      <button
        type="button"
        aria-label="Thumbnails"
        aria-pressed={tab === 'thumbs'}
        class={seg(tab === 'thumbs')}
        onclick={() => setPanelView('thumbs')}
      >
        <LayoutGrid size={14} />
      </button>
      <button
        type="button"
        aria-label="Outline"
        aria-pressed={tab === 'outline'}
        class={seg(tab === 'outline')}
        onclick={() => setPanelView('outline')}
      >
        <List size={14} />
      </button>
      <button
        type="button"
        aria-label="Annotations"
        aria-pressed={tab === 'annotations'}
        class={seg(tab === 'annotations')}
        onclick={() => setPanelView('annotations')}
      >
        <Highlighter size={14} />
      </button>
    </div>
  </div>
  {#if tab === 'thumbs'}
    <!-- Bound wrapper: the positioning effect sets the pane's scrollTop via
         thumbsWrap.firstElementChild (ThumbnailsPane's root scroll container,
         whose inline height:100% resolves against this wrapper). -->
    <div bind:this={thumbsWrap} class="min-h-0 flex-1">
      <ThumbnailsPane {documentId}>
        {#snippet children(m)}
          <button
            type="button"
            aria-label={`Page ${m.pageIndex + 1}`}
            onclick={() => jump(m.pageIndex)}
            style:position="absolute"
            style:top="{m.top}px"
            style:left="50%"
            style:transform="translateX(-50%)"
            style:width="{m.width}px"
            style:height="{m.wrapperHeight}px"
          >
            <ThumbImg
              {documentId}
              meta={m}
              class={`rounded border ${
                scroll.state.currentPage === m.pageIndex + 1
                  ? 'border-amber-700 ring-2 ring-amber-700/40 dark:border-amber-500 dark:ring-amber-500/40'
                  : 'border-stone-200 dark:border-stone-700'
              }`}
            />
            <span
              class="block pt-0.5 text-center text-[10px] text-stone-500 dark:text-stone-400"
              style:height="{m.labelHeight}px"
            >
              {m.pageIndex + 1}
            </span>
          </button>
        {/snippet}
      </ThumbnailsPane>
    </div>
  {:else if tab === 'outline'}
    <PdfOutline {documentId} />
  {:else if tab === 'annotations'}
    <PdfAnnotations {documentId} />
  {/if}
</div>
