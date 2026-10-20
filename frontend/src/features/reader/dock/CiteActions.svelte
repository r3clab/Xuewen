<script lang="ts">
  import { Copy, Download } from 'lucide-svelte';
  import { paperExportUrl } from '$features/library/api';
  import { copyCitation } from '$features/library/cite';
  import { bibFormat } from '$features/library/state.svelte';
  import { toast } from '$shared/ui/toasts.svelte';

  let { id, citeKey }: { id: string; citeKey: string | null } = $props();

  let copyError = $state(false);
  async function doCopy() {
    copyError = false;
    try {
      await copyCitation(id);
      toast('success', 'Citation copied');
    } catch {
      // Both clipboard paths failed — surface it inline so the user knows
      // to use Download (a toast alone would vanish).
      copyError = true;
    }
  }
</script>

<div class="flex items-center gap-2">
  <select
    bind:value={bibFormat.value}
    aria-label="Citation format"
    class="rounded-lg border border-stone-200 bg-parchment px-2 py-1 text-xs dark:border-stone-700 dark:bg-stone-800"
  >
    <option value="bibtex">BibTeX</option>
    <option value="biblatex">BibLaTeX</option>
  </select>
  <button
    type="button"
    onclick={doCopy}
    class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-amber-700 hover:bg-amber-700/10 dark:border-stone-700 dark:text-amber-500"
  >
    <Copy size={12} /> Copy
  </button>
  <a
    href={paperExportUrl(id, bibFormat.value)}
    download={`${citeKey ?? id}.bib`}
    class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-amber-700 hover:bg-amber-700/10 dark:border-stone-700 dark:text-amber-500"
  >
    <Download size={12} /> Download
  </a>
</div>
{#if copyError}
  <p class="mt-1 text-xs text-yellow-700 dark:text-yellow-400">Couldn't copy — use Download instead.</p>
{/if}
