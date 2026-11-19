<script lang="ts">
  import { DocumentGate, DocumentScope } from '@embedpdf/svelte/runtime';
  import PdfDocument from './PdfDocument.svelte';
  import PdfFallback from './PdfFallback.svelte';
  import TranslateBubble from '$features/reader/translation/TranslateBubble.svelte';
  import Spinner from '$shared/ui/Spinner.svelte';
  import { onPdfSelectionSettled, pdfSelectionFetchPending } from '$features/reader/pdfCopy';
  import { appSettings } from '$shared/api/settings.svelte';
  import { requestTranslate, translateTrigger } from '$features/reader/translation/state.svelte';

  // One paper's view inside the shared viewer (see PdfViewer/PdfDeck): one
  // PdfPages is mounted per open tab, scoped to its own document.
  let { documentId }: { documentId: string } = $props();

  // Selection → translate wiring — see the effect below.
  let lastPointer = $state<{ x: number; y: number } | null>(null);
  let bubble = $state<{ x: number; y: number; text: string } | null>(null);
  // Plain fields, not $state: read and written only inside event handlers.
  let pointerDown = false;
  let parkedText: string | null = null;

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
  // feature is on — NOT to the plugin's onCommitted alone, which misses a
  // selection that keeps changing after the release or pauses mid-drag (see
  // SETTLE_MS in pdfCopy.ts); the copier's trailing debounce on onChanged
  // catches those, and its cache means no second PDFium round-trip for text
  // it already fetched. The feed is
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

<svelte:window
  onpointerdowncapture={() => (pointerDown = true)}
  onpointerupcapture={(e) => {
    // Anchor first: a parked translate fired below must pop at this release.
    lastPointer = { x: e.clientX, y: e.clientY };
    pointerDown = false;
    const parked = parkedText;
    parkedText = null;
    // A pending settle/commit fetch means the selection kept changing after
    // the parked text was fetched: it is stale, and that fetch will announce
    // the final text with the pointer now up and fire the translation itself
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

<DocumentScope id={documentId}>
  <DocumentGate>
    {#snippet fallback()}
      <!-- Centered, not a corner note: the blank page area otherwise reads as
           broken during the multi-second worker boot + first parse. -->
      <div class="flex h-full items-center justify-center">
        <Spinner label="Loading document…" />
      </div>
    {/snippet}
    <!-- A file the engine can't open — missing, unparseable, or wanting a
         password the reader has no prompt for — still deserves open/download
         links, not a dead-end message. -->
    {#snippet error()}
      <PdfFallback id={documentId} />
    {/snippet}
    {#snippet locked()}
      <PdfFallback id={documentId} />
    {/snippet}
    <PdfDocument {documentId} />
  </DocumentGate>
</DocumentScope>
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
