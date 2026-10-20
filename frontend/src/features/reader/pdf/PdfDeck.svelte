<script lang="ts">
  import { onDestroy } from 'svelte';
  import { copyText } from '$shared/browser/clipboard';
  import { viewer } from '$features/reader/tabs/state.svelte';
  import { pdfUrl } from './api';
  import { useDocumentManagerCapability } from '@embedpdf/plugin-document-manager/svelte';
  import { useSelectionCapability } from '@embedpdf/plugin-selection/svelte';
  import { useAnnotationCapability } from '@embedpdf/plugin-annotation/svelte';
  import { useHistoryCapability } from '@embedpdf/plugin-history/svelte';
  import { applyToolDefaults } from '$features/reader/annotations/adapter';
  import {
    createAnnotationCommands,
    registerAnnotationCommands,
    unregisterAnnotationCommands,
  } from '$features/reader/annotations/commands';
  import { annotationTools } from '$features/reader/annotations/state.svelte';
  import { colorHex } from '$features/reader/annotations/palette';
  import {
    DocumentOpenError,
    documentsToAdopt,
    openDocumentFully,
    planOpens,
    reconcileDocuments,
  } from './deck';
  import {
    createPdfCopy,
    forgetPdfSelection,
    registerPdfCopy,
    unregisterPdfCopy,
  } from '$features/reader/pdfCopy';
  import { runWhenIdle } from './idle';
  import { toast } from '$shared/ui/toasts.svelte';
  import PdfTab from './PdfTab.svelte';
  import CitationPopover from '$features/reader/citations/CitationPopover.svelte';

  // Runs inside <EmbedPDF>, so the document-manager capability resolves against
  // the shared registry. Each open paper tab becomes a document here.
  const dm = useDocumentManagerCapability();
  type DocumentManager = NonNullable<typeof dm.provides>;

  // Three registry-wide wirings live HERE rather than in PdfPages: the
  // selection, annotation and history capabilities are each shared by every open
  // document, whereas PdfPages is mounted once per open tab and kept alive
  // behind visibility:hidden — up to maxDocuments of them would each subscribe,
  // each firing a redundant PDFium round-trip for one selection or re-pushing
  // the same five tool defaults on every color change. Both wirings that publish
  // a module-level registration tear it down BY IDENTITY, because there is one
  // live PdfDeck but not one mount: <EmbedPDF> destroys and remounts this
  // component once the plugins are ready (see documentsToAdopt), and a blind
  // clear from the outgoing instance would silently kill the live one if that
  // teardown ever landed after the replacement registered.

  // ⌘C for the reader — see reader/pdfCopy.ts for why the app has to do this
  // itself rather than leave it to the browser or the plugin.
  const selectionCap = useSelectionCapability();
  $effect(() => {
    const cap = selectionCap.provides;
    if (!cap) return;
    const copier = createPdfCopy({
      // An adapter rather than `cap` itself: the capability's event hooks are
      // typed EventHook<T>, a union of two call signatures, which does not
      // assign to the plain methods SelectionLike declares.
      selection: {
        getSelectedText: (id) => cap.getSelectedText(id),
        onBeginSelection: (h) => cap.onBeginSelection(h),
        onSelectionChange: (h) => cap.onSelectionChange(h),
        onEndSelection: (h) => cap.onEndSelection(h),
      },
      activeDocumentId: () => viewer.activeId,
      copy: copyText,
      clearNativeSelection: () => document.getSelection()?.removeAllRanges(),
      onError: (message) => toast('error', message),
    });
    registerPdfCopy(copier);
    return () => {
      unregisterPdfCopy(copier);
      copier.destroy();
    };
  });

  // The palette color → tool defaults push; the plugin keeps tool defaults
  // globally, one set shared by every open document.
  const annotationCap = useAnnotationCapability();
  $effect(() => {
    const cap = annotationCap.provides;
    if (!cap) return;
    applyToolDefaults(cap, colorHex(annotationTools.color));
  });

  // Delete / undo / redo for the global keymap and the page's selection menu
  // (see annotations/commands.ts). `activeDocumentId` is read at call time,
  // so a keystroke always acts on the tab it belongs to.
  const historyCap = useHistoryCapability();
  $effect(() => {
    const marks = annotationCap.provides;
    const history = historyCap.provides;
    if (!marks || !history) return;
    const commands = createAnnotationCommands({
      marks: (id) => marks.forDocument(id),
      history: (id) => history.forDocument(id),
      activeDocumentId: () => viewer.activeId,
    });
    registerAnnotationCommands(commands);
    return () => unregisterAnnotationCommands(commands);
  });

  // Documents we've asked the manager to open. Plain (non-reactive) set used to
  // diff against `viewer.tabs` so we open/close each document exactly once. A
  // cache of the registry's own list, NOT the source of truth — see
  // documentsToAdopt, which repairs it after the startup remount.
  const opened = new Set<string>();

  // Background tabs waiting their turn — see planOpens for why they wait.
  const pending: string[] = [];
  let draining = false;
  let cancelIdle: (() => void) | null = null;

  /// Set on teardown and checked before every open, because onDestroy on its
  /// own cannot stop this queue: it can only cancel an idle callback that is
  /// already scheduled, while an open this instance already started keeps
  /// running to completion and its `done` re-enters drain, which schedules a
  /// FRESH callback past the point onDestroy could reach. The outgoing instance
  /// would then work through its own stale `pending` — a queue the replacement
  /// has already drained — and open each background tab a second time, whose
  /// second setAnnotations wipes the marks the first load imported. That is the
  /// bug documentsToAdopt fixes for the active tab, one tab over.
  let destroyed = false;

  // openDocumentFully (pdf/deck.ts) owns the outer-task-resolves-
  // synchronously trap and rejects with the failing phase. Only an 'open'
  // failure (maxDocuments cap hit, or the document errored before an id was
  // assigned) rolls back, so a later effect run (the next tab change) retries
  // instead of leaving the tab stranded with no document; a 'load' failure
  // keeps the id in `opened` — retrying a broken PDF on every tab change
  // would loop forever.
  function open(cap: DocumentManager, id: string, done: () => void): void {
    if (destroyed) return;
    opened.add(id);
    openDocumentFully(cap, { url: pdfUrl(id), documentId: id }).then(
      () => done(),
      (e) => {
        if (e instanceof DocumentOpenError && e.phase === 'open') opened.delete(id);
        done();
      },
    );
  }

  /// Open one queued background document per idle moment, in tab order.
  /// Sequential and idle-gated, not a burst: each open is PDFium work on the
  /// shared engine lane plus a page-raster storm on the main thread, and firing
  /// several at once is exactly what jammed the renderer on a restored session.
  function drain(cap: DocumentManager): void {
    if (destroyed || draining || cancelIdle) return;
    const id = pending.shift();
    if (id === undefined) return;
    draining = true;
    cancelIdle = runWhenIdle(() => {
      cancelIdle = null;
      // It may have been closed while queued, or switched to and therefore
      // already opened ahead of its turn by the effect below.
      if (!viewer.tabs.some((t) => t.id === id) || opened.has(id)) {
        draining = false;
        drain(cap);
        return;
      }
      open(cap, id, () => {
        draining = false;
        drain(cap);
      });
    });
  }

  // Open a document for every new tab and close documents whose tab is gone.
  // Per-document scroll/zoom lives in the plugin store keyed by documentId, so
  // switching the active document (below) preserves each tab's position.
  $effect(() => {
    const cap = dm.provides;
    if (!cap) return;
    const tabIds = viewer.tabs.map((t) => t.id);
    // `opened` is not to be trusted on its own: <EmbedPDF> destroys and
    // remounts this component once the plugins are ready, handing the new
    // instance an empty set. Adopt what the registry — which survives that —
    // already holds, or the active paper is opened a second time and the second
    // load wipes the marks the first one imported (see documentsToAdopt).
    for (const id of documentsToAdopt(opened, tabIds, cap.getDocumentOrder())) opened.add(id);
    const { toOpen, toClose } = reconcileDocuments(opened, tabIds);
    // Closed first: it frees slots against maxDocuments before we ask for more.
    for (const id of toClose) {
      opened.delete(id);
      // The plugin emits no selection-change on close, so this is the only
      // thing bounding the copy cache to the set of open tabs.
      forgetPdfSelection(id);
      cap.closeDocument(id);
    }

    const { now, deferred } = planOpens(toOpen, viewer.activeId);
    for (const id of deferred) {
      if (!pending.includes(id)) pending.push(id);
    }
    // Switching to a tab whose open was still queued re-runs this effect with
    // that tab active, so it lands in `now` and opens at once — its stale queue
    // entry is skipped when the drain reaches it.
    if (now.length === 0) {
      drain(cap);
      return;
    }
    for (const id of now) {
      // Background tabs start only once the visible document is parsed, and
      // then only at idle. A failed open still drains, or one broken PDF would
      // strand every other tab unopened.
      open(cap, id, () => drain(cap));
    }
  });

  // Keep the manager's active document in sync with the active tab.
  // setActiveDocument throws if the document isn't open yet (e.g. still
  // loading, or its open was rejected above), so guard with isDocumentOpen.
  $effect(() => {
    const cap = dm.provides;
    if (cap && viewer.activeId && cap.isDocumentOpen(viewer.activeId)) {
      cap.setActiveDocument(viewer.activeId);
    }
  });

  onDestroy(() => {
    destroyed = true;
    cancelIdle?.();
    cancelIdle = null;
  });
</script>

<!-- One persistent PdfTab per open tab, hidden unless active: switching tabs is
     a show/hide, never a remount, so scroll/page/zoom survive the switch. -->
<div class="relative h-full w-full">
  {#each viewer.tabs as tab (tab.id)}
    <PdfTab id={tab.id} active={tab.id === viewer.activeId} />
  {/each}
  <CitationPopover />
</div>
