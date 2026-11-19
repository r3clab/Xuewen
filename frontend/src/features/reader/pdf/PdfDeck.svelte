<script lang="ts">
  import { onDestroy } from 'svelte';
  import { useDocuments, useDocumentsState, useKernel } from '@embedpdf/svelte/runtime';
  import { AnnotationToken } from '@embedpdf/svelte/annotation';
  import { HistoryToken } from '@embedpdf/svelte/history';
  import { copyText } from '$shared/browser/clipboard';
  import { viewer } from '$features/reader/tabs/state.svelte';
  import { pdfUrl } from './api';
  import {
    createAnnotationCommands,
    registerAnnotationCommands,
    unregisterAnnotationCommands,
  } from '$features/reader/annotations/commands';
  import { planOpens, reconcileDocuments } from './deck';
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

  // Each open paper tab becomes a document of the one kernel.
  const kernel = useKernel();
  const documents = useDocuments();
  const documentsState = useDocumentsState();

  // ⌘C for the reader — see reader/pdfCopy.ts for why the app has to do this
  // itself. Registered before any tab mounts, so every document's view finds
  // it when it attaches its selection.
  const copier = createPdfCopy({
    activeDocumentId: () => viewer.activeId,
    copy: copyText,
    clearNativeSelection: () => document.getSelection()?.removeAllRanges(),
    onError: (message) => toast('error', message),
  });
  registerPdfCopy(copier);

  // Delete / undo / redo for the global keymap and the page's selection menu
  // (see annotations/commands.ts). `activeDocumentId` is read at call time,
  // so a keystroke always acts on the tab it belongs to.
  const commands = createAnnotationCommands({
    marks: (id) => kernel.tryCapability(AnnotationToken, id),
    history: (id) => kernel.tryCapability(HistoryToken, id),
    activeDocumentId: () => viewer.activeId,
  });
  registerAnnotationCommands(commands);

  // Documents we've asked the kernel to open. Plain (non-reactive) set used to
  // diff against `viewer.tabs` so we open/close each document exactly once.
  const opened = new Set<string>();

  // Background tabs waiting their turn — see planOpens for why they wait.
  const pending: string[] = [];
  let draining = false;
  let cancelIdle: (() => void) | null = null;

  /// Set on teardown and checked before every open, because onDestroy on its
  /// own cannot stop this queue: it can only cancel an idle callback that is
  /// already scheduled, while an open this instance already started keeps
  /// running to completion and its `done` re-enters drain, which would
  /// schedule a FRESH callback past the point onDestroy could reach.
  let destroyed = false;

  // A failed open keeps the id in `opened`: retrying a broken PDF on every tab
  // change would loop forever, and the tab shows the failure instead.
  function open(id: string, done: () => void): void {
    if (destroyed) return;
    opened.add(id);
    documents.open({ kind: 'url', url: pdfUrl(id), id }, { activate: false }).then(done, done);
  }

  /// Open one queued background document per idle moment, in tab order.
  /// Sequential and idle-gated, not a burst: each open is PDFium work on the
  /// shared worker plus a page-raster storm on the main thread, and firing
  /// several at once is exactly what jammed the renderer on a restored session.
  function drain(): void {
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
        drain();
        return;
      }
      open(id, () => {
        draining = false;
        drain();
      });
    });
  }

  // Open a document for every new tab and close documents whose tab is gone.
  // Each document keeps its own camera, so switching the active document
  // (below) preserves each tab's position.
  $effect(() => {
    const tabIds = viewer.tabs.map((t) => t.id);
    const { toOpen, toClose } = reconcileDocuments(opened, tabIds);
    for (const id of toClose) {
      opened.delete(id);
      // The closing document's selection never reports a clear, so this is
      // the only thing bounding the copy cache to the set of open tabs.
      forgetPdfSelection(id);
      void documents.close(id);
    }

    const { now, deferred } = planOpens(toOpen, viewer.activeId);
    for (const id of deferred) {
      if (!pending.includes(id)) pending.push(id);
    }
    // Switching to a tab whose open was still queued re-runs this effect with
    // that tab active, so it lands in `now` and opens at once — its stale queue
    // entry is skipped when the drain reaches it.
    if (now.length === 0) {
      drain();
      return;
    }
    for (const id of now) {
      // Background tabs start only once the visible document is parsed, and
      // then only at idle. A failed open still drains, or one broken PDF would
      // strand every other tab unopened.
      open(id, () => drain());
    }
  });

  // Keep the kernel's active document in sync with the active tab: the find
  // shortcut, the export and anything else outside a document scope talk to
  // it. A document still opening can't be made active yet; this runs again
  // once it is ready.
  const activeReady = $derived.by(() => {
    const id = viewer.activeId;
    return id !== null && documentsState.documents.some((d) => d.id === id && d.status === 'ready') ? id : null;
  });
  $effect(() => {
    if (activeReady) documents.setActive(activeReady);
  });

  onDestroy(() => {
    destroyed = true;
    cancelIdle?.();
    cancelIdle = null;
    unregisterAnnotationCommands(commands);
    unregisterPdfCopy(copier);
    copier.destroy();
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
