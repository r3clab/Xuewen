<script lang="ts">
  import { pdfUrl } from './api';

  let { id }: { id: string } = $props();
  const href = $derived(pdfUrl(id));
</script>

<div class="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
  <p class="text-sm text-stone-600 dark:text-stone-300">Couldn't display this PDF.</p>
  <div class="flex gap-2">
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      class="rounded-lg border border-stone-200 px-3 py-1 text-xs font-medium text-amber-700 hover:bg-amber-700/10 dark:border-stone-700 dark:text-amber-500"
    >
      Open in new tab
    </a>
    <a
      href={href}
      download
      class="rounded-lg border border-stone-200 px-3 py-1 text-xs font-medium text-stone-600 hover:bg-parchment dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800"
    >
      Download
    </a>
  </div>
</div>
