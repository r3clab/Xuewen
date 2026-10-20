<script lang="ts">
  import { Upload } from 'lucide-svelte';
  import { openImport } from '$features/importing/queue.svelte';
  import { library } from '$features/library/state.svelte';
  import { projects } from '$features/library/organization/state.svelte';
  import { anyFilterActive, filters, noMatchesBlame } from '$features/library/search/state.svelte';
  import { clearFilters } from '$features/library/search/actions';
  import SealMark from '$shared/ui/SealMark.svelte';

  const filteredEmpty = $derived(library.papers.length === 0 && anyFilterActive());
  // The one dead-end worth a tip: a selected project that has no papers yet.
  const emptyProject = $derived(
    filters.project !== 'all' &&
      (projects.items.find((p) => p.id === filters.project)?.paper_count ?? -1) === 0,
  );
</script>

<div class="flex flex-1 flex-col items-center justify-center gap-4 p-8 text-center">
  <SealMark size={48} />
  <h2 class="font-serif text-2xl font-semibold text-ink dark:text-stone-100">Xuewen</h2>
  {#if filteredEmpty}
    <p class="max-w-sm text-sm text-stone-500 dark:text-stone-400">{noMatchesBlame(projects.items)}</p>
    {#if emptyProject}
      <p class="max-w-sm text-xs text-stone-400 dark:text-stone-500">
        This project is empty — select rows in the library table and use “Add to project…”,
        or add papers from a paper's Details pane.
      </p>
    {/if}
    <button
      type="button"
      onclick={() => void clearFilters()}
      class="rounded-lg border border-stone-200 px-3 py-1.5 text-sm hover:bg-parchment dark:border-stone-700 dark:hover:bg-stone-800"
    >Clear filters</button>
  {:else if library.papers.length === 0}
    <p class="max-w-sm text-sm text-stone-500 dark:text-stone-400">
      Your library is empty. Import a PDF, a DOI, or an arXiv link to begin.
    </p>
    <button
      type="button"
      onclick={openImport}
      class="inline-flex items-center gap-1.5 rounded-lg bg-amber-700 px-3 py-1.5 text-sm font-medium text-white hover:bg-amber-800 dark:bg-amber-600 dark:hover:bg-amber-500"
    >
      <Upload size={16} /> Import papers
    </button>
  {:else}
    <p class="max-w-sm text-sm text-stone-500 dark:text-stone-400">
      Click a paper to read it. Press <kbd class="rounded border border-stone-300 px-1 dark:border-stone-700">i</kbd> for its details.
    </p>
    <dl class="grid grid-cols-[auto_auto] gap-x-3 gap-y-1 text-xs text-stone-400 dark:text-stone-500">
      <dt><kbd class="rounded border border-stone-300 px-1 dark:border-stone-700">/</kbd></dt>
      <dd class="text-left">search</dd>
      <dt><kbd class="rounded border border-stone-300 px-1 dark:border-stone-700">Space f</kbd></dt>
      <dd class="text-left">find a paper</dd>
      <dt><kbd class="rounded border border-stone-300 px-1 dark:border-stone-700">z</kbd></dt>
      <dd class="text-left">zen mode while reading</dd>
    </dl>
  {/if}
</div>
