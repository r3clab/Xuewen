<script lang="ts">
  import { untrack } from 'svelte';
  import { RenderLayer } from '@embedpdf/svelte/render';
  import { Scrollbar, Stage, useStage, useStageState } from '@embedpdf/svelte/stage';
  import { Highlighter, LayoutGrid, List } from '@lucide/svelte';
  import { panelWidth, reader, setPanelView } from '$features/reader/state.svelte';
  import { ThumbsToken } from './engine';
  import PdfOutline from './PdfOutline.svelte';
  import PdfAnnotations from '$features/reader/annotations/PdfAnnotations.svelte';

  let { documentId }: { documentId: string } = $props();
  const main = useStage();
  const view = useStageState();
  const strip = useStage(ThumbsToken);

  // While the close animation runs, reader.panel is already null but the
  // panel is still visible — keep showing the last-used view as it slides
  // away instead of blanking.
  const tab = $derived(reader.panel ?? reader.lastPanel);

  const seg = (active: boolean) =>
    `flex flex-1 items-center justify-center rounded-md px-2 py-1 ${
      active
        ? 'bg-parchment text-ink dark:bg-stone-800 dark:text-stone-100'
        : 'text-stone-500 hover:text-ink dark:text-stone-400 dark:hover:text-stone-100'
    }`;

  // The strip starts at the current page ONCE per thumbs-view activation
  // (the strip is {#if}-gated, so this component's mount or a switch to the
  // view), and never follows the reader afterwards: that would fight the user
  // browsing the strip.
  $effect(() => {
    if (tab !== 'thumbs') return;
    untrack(() => strip.goToPage(view.currentPageIndex, { behavior: 'instant' }));
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
    <!-- A second view of the document with its own camera, so browsing the
         strip never moves the page being read. -->
    <Stage token={ThumbsToken} class="min-h-0 flex-1">
      <RenderLayer thumbnail />
      {#snippet pageChrome(page)}
        <button
          type="button"
          aria-label={`Page ${page.pageIndex + 1}`}
          onclick={() => main.goToPage(page.pageIndex)}
          class={`absolute inset-x-0 top-0 rounded border ${
            view.currentPageIndex === page.pageIndex
              ? 'border-amber-700 ring-2 ring-amber-700/40 dark:border-amber-500 dark:ring-amber-500/40'
              : 'border-stone-200 dark:border-stone-700'
          }`}
          style:bottom="{page.frame.bottom}px"
        ></button>
        <span
          class="pointer-events-none absolute inset-x-0 bottom-0 pt-0.5 text-center text-[10px] text-stone-500 dark:text-stone-400"
          style:height="{page.frame.bottom}px"
        >
          {page.pageIndex + 1}
        </span>
      {/snippet}
      {#snippet overlay()}
        <Scrollbar axis="y" />
      {/snippet}
    </Stage>
  {:else if tab === 'outline'}
    <PdfOutline />
  {:else if tab === 'annotations'}
    <PdfAnnotations {documentId} />
  {/if}
</div>
