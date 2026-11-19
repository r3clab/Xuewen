<script lang="ts">
  import { ChevronDown, ChevronLeft, ChevronRight, Contrast, Eclipse, PanelLeft, Search, SunDim, ZoomIn, ZoomOut } from '@lucide/svelte';
  import { useStage, useStageState } from '@embedpdf/svelte/stage';
  import AnnotationHistory from '$features/reader/annotations/AnnotationHistory.svelte';
  import AnnotationTools from '$features/reader/annotations/AnnotationTools.svelte';
  import ToolbarMenu from './ToolbarMenu.svelte';
  import { escapeLayer } from '$shared/ui/escape';
  import { rovingToolbar } from '$shared/ui/rovingToolbar';
  import { viewer } from '$features/reader/tabs/state.svelte';
  import { cyclePdfAppearance, nextPdfAppearance, pdfAppearance } from '$shared/ui/theme.svelte';
  import { reader, setFind, toggleSidebar } from '$features/reader/state.svelte';
  import { clampPage } from './pageNav';
  import { formatScale, isActivePreset, ZOOM_PRESETS } from './zoomPresets';
  import { activeBtn, btn, pillMotionStyle } from './pillStyles';
  import type { PillHide } from './pillHide.svelte';

  let { documentId, pill }: { documentId: string; pill: PillHide } = $props();

  const stage = useStage();
  const view = useStageState();

  // The page-number input shows the current page except while it is being
  // edited, when it shows the draft.
  let pageDraft = $state<string | null>(null);
  const pageText = $derived(pageDraft ?? String(view.currentPageIndex + 1));
  let pageInput: HTMLInputElement;
  function commitPage(): void {
    const n = clampPage(pageText, view.pageCount);
    if (n !== null) stage.goToPage(n - 1);
    pageInput.blur();
  }

  let zoomMenuOpen = $state(false);
  let annotationMenuOpen = $state(false);

  /// Whether an interaction here (editing the page number, an open menu)
  /// holds the pills visible against the auto-hide.
  export function held(): boolean {
    return pageDraft !== null || zoomMenuOpen || annotationMenuOpen;
  }

  const title = $derived(viewer.tabs.find((t) => t.id === documentId)?.title ?? '');
  const panel = $derived(reader.panel);

  const APPEARANCE_NAMES = { normal: 'Normal', dim: 'Dimmed', invert: 'Inverted' } as const;
  const appearanceLabel = $derived(APPEARANCE_NAMES[pdfAppearance.mode]);
  const nextAppearanceLabel = $derived(APPEARANCE_NAMES[nextPdfAppearance()]);
</script>

<div
  {@attach rovingToolbar}
  role="toolbar"
  aria-label="PDF controls"
  tabindex="-1"
  onpointerenter={() => pill.pillEnter()}
  onpointerleave={() => pill.pillLeave()}
  onfocusin={() => pill.focusIn()}
  onfocusout={() => pill.focusOut()}
  style={pillMotionStyle(pill.toolbarVisible)}
  class={`absolute left-1/2 top-3 z-20 flex -translate-x-1/2 items-center gap-1 rounded-xl border border-stone-200 bg-paper/90 px-1.5 py-1 shadow backdrop-blur dark:border-stone-800 dark:bg-soot/90 ${
    pill.toolbarVisible ? 'opacity-100' : 'pointer-events-none opacity-0'
  }`}
>
  {#if viewer.zen}
    <span class="max-w-48 truncate px-1 font-serif text-sm text-ink dark:text-stone-100">{title}</span>
    <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>
  {/if}

  <button
    type="button"
    class={panel ? activeBtn : btn}
    aria-label="Toggle sidebar"
    aria-expanded={panel !== null}
    title="Sidebar"
    onclick={() => toggleSidebar()}
  >
    <PanelLeft size={16} />
  </button>

  <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>

  <button
    type="button"
    class={btn}
    aria-label="Previous page"
    title="Previous page"
    disabled={view.currentPageIndex <= 0}
    onclick={() => stage.previousPage()}
  >
    <ChevronLeft size={16} />
  </button>
  <input
    class="w-9 rounded-md border border-transparent bg-transparent text-center text-sm text-stone-700 focus:border-stone-300 focus:outline-none dark:text-stone-200 dark:focus:border-stone-700"
    aria-label="Page number"
    bind:this={pageInput}
    {@attach escapeLayer(() => pageInput.blur(), { active: () => pageDraft !== null })}
    value={pageText}
    oninput={(e) => (pageDraft = e.currentTarget.value)}
    onfocus={(e) => {
      pageDraft = pageText;
      e.currentTarget.select();
    }}
    onblur={() => (pageDraft = null)}
    onkeydown={(e) => {
      if (e.key === 'Enter') commitPage();
    }}
  />
  <span class="text-sm text-stone-500 dark:text-stone-400">/ {view.pageCount}</span>
  <button
    type="button"
    class={btn}
    aria-label="Next page"
    title="Next page"
    disabled={view.currentPageIndex >= view.pageCount - 1}
    onclick={() => stage.nextPage()}
  >
    <ChevronRight size={16} />
  </button>

  <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>

  <button type="button" class={btn} aria-label="Zoom out" title="Zoom out" onclick={() => stage.zoomOut()}>
    <ZoomOut size={16} />
  </button>
  <ToolbarMenu bind:open={zoomMenuOpen} label="Zoom presets" panelClass="w-28 p-1">
    {#snippet trigger()}
      <button
        type="button"
        class={`${zoomMenuOpen ? activeBtn : btn} flex items-center gap-0.5 text-sm tabular-nums`}
        aria-label="Zoom level"
        aria-expanded={zoomMenuOpen}
        onclick={() => (zoomMenuOpen = !zoomMenuOpen)}
      >
        {formatScale(view.zoomLevel)}
        <ChevronDown size={12} />
      </button>
    {/snippet}
    {#each ZOOM_PRESETS as p (p.label)}
      <button
        type="button"
        role="menuitemradio"
        aria-checked={isActivePreset(p, view)}
        class={`block w-full rounded-lg px-2 py-1 text-left text-xs ${
          isActivePreset(p, view)
            ? 'bg-amber-700/10 text-amber-700 dark:bg-amber-500/15 dark:text-amber-500'
            : 'text-stone-600 hover:bg-parchment hover:text-ink dark:text-stone-300 dark:hover:bg-stone-800'
        }`}
        onclick={() => {
          stage.zoomTo(typeof p.level === 'number' ? p.level : { mode: p.level });
          zoomMenuOpen = false;
        }}
      >
        {p.label}
      </button>
    {/each}
  </ToolbarMenu>
  <button type="button" class={btn} aria-label="Zoom in" title="Zoom in" onclick={() => stage.zoomIn()}>
    <ZoomIn size={16} />
  </button>

  <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>

  <AnnotationTools bind:menuOpen={annotationMenuOpen} />
  <AnnotationHistory />

  <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>

  <!-- Only offered in dark mode (hidden dark:inline-flex): the dim/invert
       CSS is .dark-scoped, so in light mode the button would do nothing. -->
  <button
    type="button"
    class={`${pdfAppearance.mode !== 'normal' ? activeBtn : btn} !hidden dark:!inline-flex`}
    aria-label={`Page appearance: ${appearanceLabel}`}
    title={`Page appearance: ${appearanceLabel} — click for ${nextAppearanceLabel}`}
    onclick={cyclePdfAppearance}
  >
    {#if pdfAppearance.mode === 'dim'}<SunDim size={16} />{:else if pdfAppearance.mode === 'invert'}<Eclipse size={16} />{:else}<Contrast size={16} />{/if}
  </button>

  <button
    type="button"
    class={reader.find[documentId] ? activeBtn : btn}
    aria-label="Find in document"
    aria-expanded={!!reader.find[documentId]}
    title="Find (⌘F)"
    onclick={() => setFind(documentId)}
  >
    <Search size={16} />
  </button>
</div>
