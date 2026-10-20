<script lang="ts">
  import { pdfUrl } from './api';
  import PdfPages from './PdfPages.svelte';
  import PdfFallback from './PdfFallback.svelte';

  let { id, active }: { id: string; active: boolean } = $props();

  // One-time HEAD check for a missing/broken PDF (id is fixed for this instance).
  let failed = $state(false);
  $effect(() => {
    const controller = new AbortController();
    fetch(pdfUrl(id), { method: 'HEAD', signal: controller.signal })
      .then((res) => {
        if (!res.ok) failed = true;
      })
      .catch(() => {
        // Only a definitive !res.ok marks the tab failed. A network-level
        // probe error (server restarting, transient offline) says nothing
        // about the PDF, and latching `failed` on it would permanently
        // replace a working viewer with the fallback; the real open surfaces
        // genuine failures via doc.isError.
      });
    return () => controller.abort();
  });
</script>

<!-- Hide inactive tabs with visibility (not display:none): display:none
     collapses the tab to 0×0, which resets EmbedPDF's virtualized layout to the
     top and makes it re-scroll on every switch. `invisible` keeps the layout, so
     each tab's Viewport (and its per-document scroll/zoom) is fully preserved. -->
<div class={`absolute inset-0 ${active ? 'z-10' : 'invisible'}`}>
  {#if failed}
    <PdfFallback {id} />
  {:else}
    <PdfPages documentId={id} />
  {/if}
</div>
