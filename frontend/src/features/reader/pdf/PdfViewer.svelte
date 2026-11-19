<script lang="ts">
  import { Viewer } from '@embedpdf/svelte/runtime';
  import { createEngine, DOCUMENT_SCOPE, IDENTITY, viewerPlugins } from './engine';
  import PdfDeck from './PdfDeck.svelte';
  import Spinner from '$shared/ui/Spinner.svelte';
  import { errorMessage } from '$shared/errors';
  import { readerRegion } from '$features/reader/scroll';

  // ONE app-level viewer: every open paper is a document of this one kernel
  // (see PdfDeck). The viewer owns the engine it is given as a factory and
  // destroys it on unmount.
  const plugins = viewerPlugins();
</script>

<div class="relative min-h-0 flex-1 bg-stone-100 dark:bg-stone-950" {@attach readerRegion}>
  <Viewer engine={createEngine} {plugins} scope={DOCUMENT_SCOPE} identity={IDENTITY}>
    {#snippet fallback()}
      <div class="flex h-full items-center justify-center">
        <Spinner label="Loading PDF engine…" />
      </div>
    {/snippet}
    {#snippet error(e)}
      <p class="p-4 text-sm text-red-600 dark:text-red-400">
        Engine failed: {errorMessage(e, 'unknown error')}
      </p>
    {/snippet}
    <PdfDeck />
  </Viewer>
</div>
