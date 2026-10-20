<script lang="ts">
  import { ChevronDown, ChevronLeft, ChevronRight, Contrast, Eclipse, PanelLeft, Search, SunDim, ZoomIn, ZoomOut } from 'lucide-svelte';
  import { useZoom } from '@embedpdf/plugin-zoom/svelte';
  import { useScroll } from '@embedpdf/plugin-scroll/svelte';
  import AnnotationHistory from '$features/reader/annotations/AnnotationHistory.svelte';
  import AnnotationTools from '$features/reader/annotations/AnnotationTools.svelte';
  import ToolbarMenu from './ToolbarMenu.svelte';
  import { viewer } from '$features/reader/tabs/state.svelte';
  import { cyclePdfAppearance, nextPdfAppearance, pdfAppearance } from '$shared/ui/theme.svelte';
  import { reader, setFind, toggleSidebar } from '$features/reader/state.svelte';
  import { clampPage } from './pageNav';
  import { formatScale, isActivePreset, ZOOM_PRESETS } from './zoomPresets';
  import { activeBtn, btn, pillMotionStyle } from './pillStyles';
  import type { PillHide } from './pillHide.svelte';

  let { documentId, pill }: { documentId: string; pill: PillHide } = $props();

  const zoom = useZoom(() => documentId);
  const scroll = useScroll(() => documentId);

  // --- page-number input: mirrors currentPage except while being edited ---
  let pageText = $state('1');
  let pageEditing = $state(false);
  let annotationMenuOpen = $state(false);
  $effect(() => {
    if (!pageEditing) pageText = String(scroll.state.currentPage);
  });
  function commitPage(target: HTMLInputElement): void {
    const n = clampPage(pageText, scroll.state.totalPages);
    if (n !== null) scroll.provides?.scrollToPage({ pageNumber: n });
    target.blur();
  }

  // --- zoom preset menu ---
  let zoomMenuOpen = $state(false);

  // Local interaction holds (page editing, zoom menu) feed the shared
  // auto-hide controller. Wrapped in $effect (not a bare top-level call)
  // because `pill` is a reactive prop — reading it outside a reactive
  // context only captures its initial value (svelte-check flags this as
  // `state_referenced_locally`).
  $effect(() => {
    pill.setExtraHold(() => pageEditing || zoomMenuOpen || annotationMenuOpen);
  });

  const title = $derived(viewer.tabs.find((t) => t.id === documentId)?.title ?? '');
  const panel = $derived(reader.panel);

  const APPEARANCE_NAMES = { normal: 'Normal', dim: 'Dimmed', invert: 'Inverted' } as const;
  const appearanceLabel = $derived(APPEARANCE_NAMES[pdfAppearance.mode]);
  const nextAppearanceLabel = $derived(APPEARANCE_NAMES[nextPdfAppearance()]);
</script>

<!-- svelte-ignore a11y_interactive_supports_focus -- every control inside
     the pill is individually tabbable via normal document tab order; the
     toolbar container itself is not a tab stop. A roving-tabindex pass for
     arrow-key navigation between controls is a filed follow-up, not done
     here. -->
<div
  role="toolbar"
  aria-label="PDF controls"
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
    disabled={scroll.state.currentPage <= 1}
    onclick={() => scroll.provides?.scrollToPreviousPage()}
  >
    <ChevronLeft size={16} />
  </button>
  <input
    class="w-9 rounded-md border border-transparent bg-transparent text-center text-sm text-stone-700 focus:border-stone-300 focus:outline-none dark:text-stone-200 dark:focus:border-stone-700"
    aria-label="Page number"
    bind:value={pageText}
    onfocus={(e) => {
      pageEditing = true;
      e.currentTarget.select();
    }}
    onblur={() => (pageEditing = false)}
    onkeydown={(e) => {
      if (e.key === 'Enter') commitPage(e.currentTarget);
      else if (e.key === 'Escape') {
        e.stopPropagation(); // the global cascade must not see this
        pageText = String(scroll.state.currentPage);
        e.currentTarget.blur();
      }
    }}
  />
  <span class="text-sm text-stone-500 dark:text-stone-400">/ {scroll.state.totalPages}</span>
  <button
    type="button"
    class={btn}
    aria-label="Next page"
    title="Next page"
    disabled={scroll.state.currentPage >= scroll.state.totalPages}
    onclick={() => scroll.provides?.scrollToNextPage()}
  >
    <ChevronRight size={16} />
  </button>

  <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>

  <button type="button" class={btn} aria-label="Zoom out" title="Zoom out" onclick={() => zoom.provides?.zoomOut()}>
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
        {formatScale(zoom.state.currentZoomLevel)}
        <ChevronDown size={12} />
      </button>
    {/snippet}
    {#each ZOOM_PRESETS as p (p.label)}
      <button
        type="button"
        role="menuitem"
        class={`block w-full rounded-lg px-2 py-1 text-left text-xs ${
          isActivePreset(p, zoom.state.currentZoomLevel)
            ? 'bg-amber-700/10 text-amber-700 dark:bg-amber-500/15 dark:text-amber-500'
            : 'text-stone-600 hover:bg-parchment hover:text-ink dark:text-stone-300 dark:hover:bg-stone-800'
        }`}
        onclick={() => {
          zoom.provides?.requestZoom(p.level);
          zoomMenuOpen = false;
        }}
      >
        {p.label}
      </button>
    {/each}
  </ToolbarMenu>
  <button type="button" class={btn} aria-label="Zoom in" title="Zoom in" onclick={() => zoom.provides?.zoomIn()}>
    <ZoomIn size={16} />
  </button>

  <span class="h-5 w-px shrink-0 bg-stone-200 dark:bg-stone-800"></span>

  <AnnotationTools {documentId} onHoldChange={(held) => (annotationMenuOpen = held)} />
  <AnnotationHistory {documentId} />

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
