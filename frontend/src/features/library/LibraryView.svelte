<script lang="ts">
  import { library } from './state.svelte';
  import FilterStrip from '$features/library/search/FilterStrip.svelte';
  import LibraryTable from '$features/library/table/LibraryTable.svelte';
  import Spinner from '$shared/ui/Spinner.svelte';
  import Welcome from '$features/library/table/Welcome.svelte';
</script>

<!-- The strip stays outside the branches: a filter that matches nothing must
     still be visible to be undone. Later loads keep the stale table up rather
     than flashing a spinner on every keystroke. -->
<section aria-label="Papers" aria-busy={library.loading} class="flex min-h-0 min-w-0 flex-1 flex-col">
  <FilterStrip />
  {#if library.error}
    <p
      role="alert"
      class="shrink-0 border-b border-stone-200 px-4 py-2 text-sm text-red-600 dark:border-stone-800 dark:text-red-400"
    >
      {library.error}
    </p>
  {/if}
  {#if !library.loaded}
    <div class="flex flex-1 items-center justify-center"><Spinner label="Loading library…" /></div>
  {:else if library.papers.length > 0}
    <LibraryTable />
  {:else if !library.error}
    <Welcome />
  {/if}
</section>
