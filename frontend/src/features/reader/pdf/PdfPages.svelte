<script lang="ts">
  import { onDestroy } from 'svelte';
  import { useRegistry, useDocumentState } from '@embedpdf/core/svelte';
  import { Viewport, useViewportCapability } from '@embedpdf/plugin-viewport/svelte';
  import { Scroller, type PageLayout } from '@embedpdf/plugin-scroll/svelte';
  import { ZoomGestureWrapper } from '@embedpdf/plugin-zoom/svelte';
  import { DocumentContent } from '@embedpdf/plugin-document-manager/svelte';
  import { RenderLayer } from '@embedpdf/plugin-render/svelte';
  import { SelectionLayer } from '@embedpdf/plugin-selection/svelte';
  import { GlobalPointerProvider, PagePointerProvider } from '@embedpdf/plugin-interaction-manager/svelte';
  import { TilingLayer } from '@embedpdf/plugin-tiling/svelte';
  import PdfToolbar from './PdfToolbar.svelte';
  import PdfQuickActions from './PdfQuickActions.svelte';
  import PdfFallback from './PdfFallback.svelte';
  import PdfFindBar from './PdfFindBar.svelte';
  import PdfSidePanel from './PdfSidePanel.svelte';
  import TranslateBubble from '$features/reader/translation/TranslateBubble.svelte';
  import Spinner from '$shared/ui/Spinner.svelte';
  import { SearchLayer } from '@embedpdf/plugin-search/svelte';
  import {
    AnnotationLayer,
    useAnnotation,
    useAnnotationCapability,
    type AnnotationSelectionMenuProps,
  } from '@embedpdf/plugin-annotation/svelte';
  import AnnotationSelectionMenu from '$features/reader/annotations/AnnotationSelectionMenu.svelte';
  import CitationLayer from '$features/reader/citations/CitationLayer.svelte';
  import { ANNOTATION_RENDERERS } from '$features/reader/annotations/renderers';
  import { createAnnotationSync } from '$features/reader/annotations/sync';
  import { toast } from '$shared/ui/toasts.svelte';
  import { runCitationPipeline } from '$features/reader/citations/pipeline';
  import type { EngineLike } from '$features/reader/citations/load';
  import { onPdfSelectionSettled, pdfSelectionFetchPending } from '$features/reader/pdfCopy';
  import { runWhenIdle } from './idle';
  import { panelWidth, reader } from '$features/reader/state.svelte';
  import { pdfAppearance } from '$shared/ui/theme.svelte';
  import { createPillHide } from './pillHide.svelte';
  import { Spring } from 'svelte/motion';
  import { springTo, SPRINGS } from '$shared/ui/motion';
  import { appSettings } from '$shared/api/settings.svelte';
  import { requestTranslate, translateTrigger } from '$features/reader/translation/state.svelte';
  import type { CitationData } from '$features/reader/citations/references';
  import type { PaperSummary } from '$shared/api/generated/PaperSummary';

  // Renders one paper's pages inside the shared <EmbedPDF> (see PdfViewer/PdfDeck).
  // Bound to its own `documentId` — one PdfPages is mounted per open tab — so the
  // shared engine is fine while each tab reads/extracts its own document.
  let { documentId }: { documentId: string } = $props();

  const ctx = useRegistry();
  const docState = useDocumentState(() => documentId);

  // Annotations ⇄ the SQLite sidecar. The tab id IS the paper id, so one sync
  // per mounted tab covers exactly one paper. Both capabilities start null and
  // settle once, so this effect runs at most twice; re-running is safe because
  // `destroy` still flushes pending writes and the replacement sync
  // re-subscribes synchronously in `start()`, so no event falls in the gap.
  //
  // Withholding the id until the document is loaded is load-bearing. The
  // plugin creates a document's annotation state in `onDocumentLoadingStarted`,
  // and `useAnnotation`'s own effect calls `scope.getState()` eagerly — handed
  // an id the plugin has not seen yet that throws "Annotation state not found
  // for document: <id>" *inside* Svelte's effect flush, which aborts the rest
  // of the flush and leaves the reader permanently blank (no page ever
  // renders). PdfPages mounts as soon as its tab exists, well before PdfDeck's
  // openDocumentUrl resolves, so it always lost that race. Passing '' takes
  // the hook's own early-return path instead: the scope stays null until the
  // document is ready, which the sync effect below already waits for.
  const annotationScope = useAnnotation(() => (docState.current?.document ? documentId : ''));
  const annotationCap = useAnnotationCapability();
  $effect(() => {
    const scope = annotationScope.provides;
    const cap = annotationCap.provides;
    if (!scope || !cap) return;
    const sync = createAnnotationSync({
      paperId: documentId,
      documentId,
      scope,
      subscribe: (handler) => cap.onAnnotationEvent(handler),
      // A mark that silently failed to save is the worst outcome here: the
      // reader sees it on the page and only finds out it was never stored on
      // the next open.
      onError: (message) => toast('error', `Annotation not saved — ${message}`),
    });
    void sync.start();
    return () => void sync.destroy();
  });

  // Selection → translate wiring (Task 7) — see the effect below.
  let lastPointer = $state<{ x: number; y: number } | null>(null);
  let bubble = $state<{ x: number; y: number; text: string } | null>(null);
  // Plain fields, not $state: read and written only inside event handlers.
  let pointerDown = false;
  let parkedText: string | null = null;

  // Shared zen auto-hide for the floating pills (see pdf/pillHide.svelte.ts).
  const pill = createPillHide(() => documentId);
  let pillHost = $state<HTMLDivElement | undefined>();
  $effect(() => {
    pill.setHost(pillHost ?? null);
  });

  // Reading direction feeds the toolbar's scroll-hide. Subscribed
  // registry-wide and filtered on the id rather than through
  // `forDocument(documentId)`, which throws for a document the viewport
  // plugin has not seen yet — PdfPages mounts before the document opens.
  // A smooth scroll is one this app commanded (jump to page, find, outline),
  // so it re-anchors instead of counting as the reader scrolling away.
  const viewportCap = useViewportCapability();
  $effect(() => {
    const cap = viewportCap.provides;
    if (!cap) return;
    return cap.onScrollChange(({ documentId: id, scrollMetrics }) => {
      if (id !== documentId) return;
      if (cap.isSmoothScrolling()) pill.onScrollJump(scrollMetrics.scrollTop);
      else pill.onScroll(scrollMetrics.scrollTop);
    });
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
  // Pure projection of the document's page geometry (PDF points, for the
  // citation overlay) — no engine call touches it, so a reactive read of the
  // $state-proxied document is fine here.
  const pageSizes = $derived(
    docState.current?.document?.pages.map((p) => ({
      width: p.size.width,
      height: p.size.height,
    })) ?? [],
  );

  // Extract citation markers + match them against the library ONCE per document.
  // `docState.current` (and its `.document`) is reassigned on any core change —
  // incl. zoom scale and an initial load→reload — so guarding on the document
  // object's identity still re-ran extraction. Guard on the (fixed) documentId
  // instead: one PdfPages is mounted per tab, so extraction runs exactly once.
  // Extraction is scheduled once per document at idle; the schedule itself is
  // a true one-shot, cancelled ONLY on component unmount. This $effect can
  // legitimately re-run on the same document (zoom/reload churn re-fires
  // `docState.current`) — such re-runs must NOT cancel a pending/in-flight
  // extraction, so this effect returns no cleanup. Failures are caught/logged
  // so the reader still works without citation hovers.
  let extractedFor: string | null = null;
  let extractionCancelled = false;
  let cancelExtractionIdle: (() => void) | null = null;
  $effect(() => {
    const registry = ctx.registry;
    const rawDoc = docState.current?.document ?? null;
    if (!registry || !rawDoc || extractedFor === documentId) return;
    extractedFor = documentId;
    const engine = registry.getEngine();
    // The document/page objects are Svelte $state proxies (EmbedPDF's core
    // wraps them for reactivity). PDFium now runs in a worker (see
    // pdf/engine.ts), and every engine call round-trips doc/page through
    // postMessage — a live Proxy throws DataCloneError there. Snapshot once
    // into plain data so the pipeline can hand it back to the worker.
    const doc = $state.snapshot(rawDoc);
    // Extraction is PDFium work (now off the main thread, in the worker) —
    // wait for idle so the first pages paint before we start crawling
    // annotations/text. The pipeline itself (phases, cancellation points,
    // progressive publishes) lives in citations/pipeline.ts.
    cancelExtractionIdle = runWhenIdle(() => {
      void runCitationPipeline(engine as unknown as EngineLike, doc, documentId, {
        isCancelled: () => extractionCancelled,
        onUpdate: (u) => {
          if (u.citations) citations = u.citations;
          if (u.matches) matches = u.matches;
        },
      });
    });
  });

  onDestroy(() => {
    extractionCancelled = true;
    cancelExtractionIdle?.();
  });

  // Auto mode fires requestTranslate; Manual mode shows the bubble instead,
  // which the user must click (see TranslateBubble.svelte). Anchored at the
  // window-level capture-phase pointerup (svelte:window handler below), not
  // page-space math — the selection plugin can stop bubbling, so a div-level
  // handler would leave lastPointer stale.
  function fireTranslate(text: string): void {
    const at = lastPointer ?? { x: window.innerWidth / 2, y: 200 };
    if (translateTrigger() === 'auto') {
      bubble = null;
      void requestTranslate(text, at);
    } else {
      bubble = { x: at.x, y: at.y, text };
    }
  }

  // Subscribe to the ⌘C copier's settled-selection feed while the translate
  // feature is on — NOT to the plugin's onEndSelection, which never fires for
  // a drag released in the gutter, past the margin, over the toolbar, or on
  // another page (see SETTLE_MS in pdfCopy.ts); the copier's trailing debounce
  // on onSelectionChange catches every one of those, and its cache means no
  // second PDFium round-trip for text it already fetched. The feed is
  // module-wide and every open tab keeps its own (hidden) PdfPages mounted, so
  // guard on documentId — only the tab that owns the selection reacts. A
  // settle can land MID-drag (a >200ms pause with the button still down),
  // which must not fire a paid auto-translation per pause: text arriving
  // while the pointer is down is parked and fired from the capture-phase
  // pointerup below (which refreshes the anchor first, and drops the parked
  // text as stale when a newer fetch is still pending); text arriving with
  // the pointer up — the common release-on-page drag, double/triple-click,
  // and a post-release settle — fires immediately.
  $effect(() => {
    if (!appSettings.translate.enabled) return;
    const unsub = onPdfSelectionSettled((docId, text) => {
      if (docId !== documentId) return;
      // The copier caches the parts '\n'-joined; translate sends prose.
      const prose = text.split('\n').join(' ').trim();
      if (!prose) {
        // The selection was cleared (e.g. a click on the page) — dismiss.
        parkedText = null;
        bubble = null;
        return;
      }
      if (pointerDown) parkedText = prose;
      else fireTranslate(prose);
    });
    return () => {
      unsub();
      parkedText = null;
      bubble = null;
    };
  });
</script>

<!-- The trash button that floats over a selected mark. Handed to every page's
     AnnotationLayer, which renders it for the selected annotation only (and
     never for a multi-selection). -->
{#snippet annotationMenu({ menuWrapperProps, context }: AnnotationSelectionMenuProps)}
  <AnnotationSelectionMenu {menuWrapperProps} {context} />
{/snippet}

{#snippet renderPage(page: PageLayout)}
  <div style:width="{page.width}px" style:height="{page.height}px" style:position="relative">
    <PagePointerProvider {documentId} pageIndex={page.pageIndex}>
      <!-- Low-res base rendered once (scale locked at 1, CSS-scaled by the
           framework); TilingLayer draws crisp visible tiles at the real zoom.
           This mirrors the ready-made viewer and is the perf fix — do NOT
           remove scale={1} or pages re-render fully on every zoom. -->
      <!-- Dark-mode dim/invert (app.css, .dark-scoped) wraps ONLY the raster
           layers: selection/search highlights and citation overlays keep
           their true colors, and nothing position:fixed lives under the
           filter (a filter creates a new containing block). -->
      <div class="pointer-events-none absolute inset-0" data-pdf-appearance={pdfAppearance.mode}>
        <RenderLayer {documentId} pageIndex={page.pageIndex} scale={1} class="pointer-events-none" />
        <TilingLayer {documentId} pageIndex={page.pageIndex} class="pointer-events-none" />
      </div>
      <SelectionLayer {documentId} pageIndex={page.pageIndex} />
      <SearchLayer
        {documentId}
        pageIndex={page.pageIndex}
        class="pointer-events-none"
        highlightColor="rgba(180, 83, 9, 0.28)"
        activeHighlightColor="rgba(180, 83, 9, 0.55)"
      />
      <CitationLayer
        {documentId}
        pageIndex={page.pageIndex}
        pageWidthPt={pageSizes[page.pageIndex]?.width ?? page.width}
        pageHeightPt={pageSizes[page.pageIndex]?.height ?? page.height}
        data={citations}
        {matches}
      />
      <!-- Outside the data-pdf-appearance wrapper, like the other overlays:
           marks keep the palette color the user picked instead of being
           dimmed or hue-rotated into some other color entirely. Renderers
           carry the link-renderer suppression (see annotations/renderers.ts) —
           CitationLayer above already owns link annotations. -->
      <!-- scale/rotation are deliberately not passed: omitted, the layer reads
           both off the document state, which is the same source the zoom
           plugin writes to. Passing them would fork that. -->
      <AnnotationLayer
        {documentId}
        pageIndex={page.pageIndex}
        annotationRenderers={ANNOTATION_RENDERERS}
        selectionMenuSnippet={annotationMenu}
      />
    </PagePointerProvider>
  </div>
{/snippet}

<svelte:window
  onpointermove={(e) => pill.onWindowMove(e)}
  onpointerdowncapture={() => (pointerDown = true)}
  onpointerupcapture={(e) => {
    // Anchor first: a parked translate fired below must pop at this release.
    lastPointer = { x: e.clientX, y: e.clientY };
    pointerDown = false;
    const parked = parkedText;
    parkedText = null;
    // A pending settle/end fetch means the selection kept changing after the
    // parked text was fetched: it is stale, and that fetch will announce the
    // final text with the pointer now up and fire the translation itself
    // (deduped by the copier if it comes back unchanged). Firing the parked
    // copy too would pay for two translations of one drag — the first with
    // text the user never finally selected.
    if (parked && !pdfSelectionFetchPending(documentId)) fireTranslate(parked);
  }}
  onpointercancelcapture={() => {
    // The drag died (touch/pen cancellation) — a parked settle firing on some
    // later, unrelated pointerup would be a surprise. Drop it.
    pointerDown = false;
    parkedText = null;
  }}
/>

<DocumentContent {documentId}>
  {#snippet children(doc)}
    {#if doc.isLoaded}
      <div class="flex h-full">
        {#if reader.panel || panelW.current > 1}
          <!-- Kept mounted while the spring settles so closing slides the
               panel away instead of blanking it; inert once logically closed.
               A rapid close→reopen within the settle window intentionally
               skips re-positioning/reveal: the panel never unmounts and `tab`
               never changes, so the user keeps their browse position — this
               is deliberate, not a missed one-shot. -->
          <div
            class="relative min-h-0 shrink-0 overflow-hidden"
            style={`width:${panelW.current}px`}
            inert={!reader.panel}
          >
            <!-- Fixed at the open view's own width, not the springing
                 wrapper's, so the panel's contents don't reflow on every
                 animation frame while it slides. -->
            <div
              class="absolute inset-y-0 left-0 flex"
              style={`width:${panelWidth(reader.panel ?? reader.lastPanel)}px`}
            >
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
          <!-- Zoom/scroll/pinch wiring mirrors EmbedPDF's own ready-made viewer
               (viewers/snippet app.tsx): GlobalPointerProvider > Viewport >
               ZoomGestureWrapper > Scroller, all stock. -->
          <GlobalPointerProvider {documentId}>
            <!-- select-none scopes native selection out of the page area only
                 (the toolbar, find bar and side panel are siblings and stay
                 selectable). PDF text selection is the plugin's synthetic
                 overlay and is unaffected; what this stops is the browser's own
                 drag-selection running alongside it and grabbing the floating
                 toolbar's page/zoom labels when a drag sweeps over them. That
                 stray DOM selection would otherwise make shortcuts.ts stand
                 aside and copy "/ 12" instead of the sentence. -->
            <Viewport {documentId} class="h-full w-full select-none">
              <!-- No class on ZoomGestureWrapper: it must size to its content,
                   not the viewport. Its pinch-anchor math reads the wrapped
                   element's own width/height, so forcing h-full/w-full (element
                   = viewport size, while the content is many pages tall) breaks
                   the anchor — pinching a corner scaled toward the opposite one.
                   EmbedPDF's own viewer passes no class here. -->
              <ZoomGestureWrapper {documentId}>
                <Scroller {documentId} {renderPage} />
              </ZoomGestureWrapper>
            </Viewport>
          </GlobalPointerProvider>
        </div>
      </div>
      {#if bubble}
        <TranslateBubble
          x={bubble.x}
          y={bubble.y}
          onTranslate={() => {
            const b = bubble;
            bubble = null;
            if (b) void requestTranslate(b.text, { x: b.x, y: b.y });
          }}
        />
      {/if}
    {:else if doc.isError}
      <!-- Same escape hatch as PdfTab's missing-PDF probe: a file that HEADs
           fine but fails to parse still deserves open/download links, not a
           dead-end message. -->
      <PdfFallback id={documentId} />
    {:else}
      <!-- Centered, not a corner note: the blank page area otherwise reads
           as broken during the multi-second worker boot + first parse. -->
      <div class="flex h-full items-center justify-center">
        <Spinner label="Loading document…" />
      </div>
    {/if}
  {/snippet}
</DocumentContent>
