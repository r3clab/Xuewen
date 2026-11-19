<script lang="ts">
  import PdfPages from './PdfPages.svelte';

  let { id, active }: { id: string; active: boolean } = $props();
</script>

<!-- Hide inactive tabs with visibility (not display:none): display:none
     collapses the tab to 0×0, which resets EmbedPDF's virtualized layout to the
     top and makes it re-scroll on every switch. `invisible` keeps the layout, so
     each tab's Stage (and its per-document scroll/zoom) is fully preserved. -->
<div class={`absolute inset-0 ${active ? 'z-10' : 'invisible'}`}>
  <PdfPages documentId={id} />
</div>
