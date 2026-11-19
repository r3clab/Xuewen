<script lang="ts">
  import { onDestroy, untrack } from 'svelte';
  import { Spring } from 'svelte/motion';
  import { useDocuments, useOptionalCapability, usePageList, type PageRef } from '@embedpdf/svelte/runtime';
  import { AnnotationLayer, AnnotationMenu, AnnotationToken, useAnnotationState } from '@embedpdf/svelte/annotation';
  import { HistoryToken } from '@embedpdf/svelte/history';
  import { RenderLayer } from '@embedpdf/svelte/render';
  import { SearchLayer } from '@embedpdf/svelte/search';
  import { SelectionLayer, SelectionToken } from '@embedpdf/svelte/selection';
  import { Scrollbar, Stage, useScrollMetrics, useStage } from '@embedpdf/svelte/stage';
  import PdfToolbar from './PdfToolbar.svelte';
  import PdfQuickActions from './PdfQuickActions.svelte';
  import PdfFindBar from './PdfFindBar.svelte';
  import PdfSidePanel from './PdfSidePanel.svelte';
  import AnnotationSelectionMenu from '$features/reader/annotations/AnnotationSelectionMenu.svelte';
  import CitationLayer from '$features/reader/citations/CitationLayer.svelte';
  import { createAnnotationSync } from '$features/reader/annotations/sync';
  import { runCitationPipeline } from '$features/reader/citations/pipeline';
  import type { CitationSource } from '$features/reader/citations/load';
  import { attachPdfSelection } from '$features/reader/pdfCopy';
  import { toast } from '$shared/ui/toasts.svelte';
  import { PageTextToken } from './pageText';
  import { runWhenIdle } from './idle';
  import { panelWidth, reader } from '$features/reader/state.svelte';
  import { pdfAppearance } from '$shared/ui/theme.svelte';
  import { createPillHide } from './pillHide.svelte';
  import { springTo, SPRINGS } from '$shared/ui/motion';
  import type { CitationData } from '$features/reader/citations/references';
  import type { PaperSummary } from '$shared/api/generated/PaperSummary';

  // One open paper's reader, mounted inside its document's scope once the
  // document is ready (see PdfPages), so every reader and layer below talks
  // to this document. The tab id IS the paper id.
  let { documentId }: { documentId: string } = $props();

  const documents = useDocuments();
  const pages = usePageList();
  const stage = useStage();
  const annotation = useOptionalCapability(AnnotationToken);
  const history = useOptionalCapability(HistoryToken);
  const pageText = useOptionalCapability(PageTextToken);
  const selection = useOptionalCapability(SelectionToken);
  const marks = useAnnotationState();
  const marksReady = $derived(marks.status === 'ready');

  const pageIndexOf = (ref: PageRef) => documents.getPageIndex(ref, documentId);

  // Annotations ⇄ the SQLite sidecar, one sync per mounted tab. It waits for
  // the document's own annotations to load: the restore imports on top of
  // them, and a mark the file already carries has to be there to be told
  // apart from ours (see annotations/sync.ts).
  $effect(() => {
    const scope = annotation.current;
    const steps = history.current;
    const text = pageText.current;
    if (!scope || !steps || !text || !marksReady) return;
    return untrack(() => {
      const sync = createAnnotationSync({
        paperId: documentId,
        scope,
        pages: {
          list: () => documents.listPages(documentId),
          indexOf: pageIndexOf,
          textUnder: (page, quads) => text.readTextUnder(page, quads),
        },
        clearHistory: () => steps.clear(),
        // A mark that silently failed to save is the worst outcome here: the
        // reader sees it on the page and only finds out it was never stored
        // on the next open.
        onError: (message) => toast('error', `Annotation not saved — ${message}`),
      });
      void sync.start();
      return () => void sync.destroy();
    });
  });

  // ⌘C and selection-translate read this document's selection through the
  // app's copier (see reader/pdfCopy.ts).
  $effect(() => {
    const source = selection.current;
    if (!source) return;
    return untrack(() => attachPdfSelection(documentId, source));
  });

  // Shared zen auto-hide for the floating pills (see pdf/pillHide.svelte.ts).
  const pill = createPillHide(() => documentId);
  let pillHost = $state<HTMLDivElement | undefined>();
  $effect(() => {
    pill.setHost(pillHost ?? null);
  });

  // Reading direction feeds the toolbar's scroll-hide. A camera tween is one
  // this app commanded (jump to page, find, outline), so it re-anchors
  // instead of counting as the reader scrolling away.
  const metrics = useScrollMetrics();
  $effect(() => {
    const top = metrics.scrollTop;
    untrack(() => (stage.isMoving() ? pill.onScrollJump(top) : pill.onScroll(top)));
  });

  // Animated panel push: the wrapper's width springs 0↔PANEL_W so the PDF
  // eases sideways instead of jumping when the panel toggles.
  // Per-view (annotations need room for prose) — see reader/state.svelte.ts.
  // svelte-ignore state_referenced_locally -- initial value only; the
  // $effect below drives every subsequent update via panelW.target.
  const panelW = new Spring(reader.panel ? panelWidth(reader.panel) : 0, SPRINGS.pane);
  $effect(() => {
    springTo(panelW, reader.panel ? panelWidth(reader.panel) : 0);
  });

  let citations = $state<CitationData>({ references: [], markers: [] });
  let matches = $state<Map<number, PaperSummary>>(new Map());

  // Extract citation markers and match them against the library ONCE per
  // document, at idle so the first pages paint before the crawl starts. The
  // link annotations it starts from are read once the document's annotations
  // are in. Failures are caught inside the pipeline, so the reader still
  // works without citation hovers.
  let extractionStarted = false;
  let extractionCancelled = false;
  let cancelExtractionIdle: (() => void) | null = null;
  $effect(() => {
    const scope = annotation.current;
    const text = pageText.current;
    if (!scope || !text || !marksReady || extractionStarted) return;
    extractionStarted = true;
    untrack(() => {
      const source: CitationSource = {
        pages: documents.listPages(documentId).map((page) => page.size),
        links: () =>
          scope.list({ subtype: 'link' }).flatMap((link) =>
            link.subtype === 'link'
              ? [{ pageIndex: pageIndexOf(link.ref.page), rect: link.rect, target: link.target }]
              : [],
          ),
        readRuns: (pageIndex) => text.readRuns(pageIndex),
        pageIndexOf,
      };
      cancelExtractionIdle = runWhenIdle(() => {
        void runCitationPipeline(source, documentId, {
          isCancelled: () => extractionCancelled,
          onUpdate: (u) => {
            if (u.citations) citations = u.citations;
            if (u.matches) matches = u.matches;
          },
        });
      });
    });
  });

  onDestroy(() => {
    extractionCancelled = true;
    cancelExtractionIdle?.();
  });
</script>

<svelte:window onpointermove={(e) => pill.onWindowMove(e)} />

<div class="flex h-full">
  {#if reader.panel || panelW.current > 1}
    <!-- Kept mounted while the spring settles so closing slides the panel
         away instead of blanking it; inert once logically closed. A rapid
         close→reopen within the settle window intentionally skips
         re-positioning/reveal: the panel never unmounts and `tab` never
         changes, so the user keeps their browse position — this is
         deliberate, not a missed one-shot. -->
    <div class="relative min-h-0 shrink-0 overflow-hidden" style={`width:${panelW.current}px`} inert={!reader.panel}>
      <!-- Fixed at the open view's own width, not the springing wrapper's, so
           the panel's contents don't reflow on every animation frame while it
           slides. -->
      <div class="absolute inset-y-0 left-0 flex" style={`width:${panelWidth(reader.panel ?? reader.lastPanel)}px`}>
        <PdfSidePanel {documentId} />
      </div>
    </div>
  {/if}
  <div class="relative min-w-0 flex-1" bind:this={pillHost}>
    <PdfToolbar {documentId} {pill} />
    <PdfQuickActions {pill} />
    {#if reader.find[documentId]}
      <PdfFindBar {documentId} />
    {/if}
    <!-- select-none scopes native selection out of the page area only (the
         toolbar, find bar and side panel are siblings and stay selectable).
         PDF text selection is the plugin's synthetic overlay and is
         unaffected; what this stops is the browser's own drag-selection
         running alongside it and grabbing the floating toolbar's page/zoom
         labels when a drag sweeps over them. That stray DOM selection would
         otherwise make shortcuts.ts stand aside and copy "/ 12" instead of
         the sentence. -->
    <Stage class="h-full w-full select-none">
      {#snippet children(page)}
        <!-- Dark-mode dim/invert (app.css, .dark-scoped) wraps ONLY the
             raster: selection/search highlights, citation overlays and marks
             keep their true colors, and nothing position:fixed lives under
             the filter (a filter creates a new containing block). -->
        <div class="pointer-events-none absolute inset-0" data-pdf-appearance={pdfAppearance.mode}>
          <RenderLayer />
        </div>
        <SelectionLayer />
        <SearchLayer />
        <CitationLayer
          pageIndex={page.pageIndex}
          pageWidthPt={pages.current[page.pageIndex]?.size.width ?? 1}
          pageHeightPt={pages.current[page.pageIndex]?.size.height ?? 1}
          data={citations}
          {matches}
        />
        <AnnotationLayer />
      {/snippet}
      {#snippet overlay()}
        <AnnotationMenu>
          <AnnotationSelectionMenu />
        </AnnotationMenu>
        <Scrollbar axis="y" />
        <Scrollbar axis="x" />
      {/snippet}
    </Stage>
  </div>
</div>
