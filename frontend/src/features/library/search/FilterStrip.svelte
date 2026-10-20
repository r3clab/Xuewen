<script lang="ts">
  import { ChevronDown, Download } from 'lucide-svelte';
  import { exportUrl } from '$features/library/api';
  import { bibFormat } from '$features/library/state.svelte';
  import { filters, relevanceSearchActive } from './state.svelte';
  import { setStatusFilter } from './actions';
  import type { StatusFilter } from './types';
  import FilterPills, { type Zone } from './FilterPills.svelte';

  // One zone at a time, folded by default: the pill rows can grow long, and a
  // count badge on the folded toggle keeps an active filter from hiding.
  let zone = $state<Zone | null>(null);
  const projectCount = $derived(filters.project !== 'all' ? 1 : 0);
  const tagCount = $derived((filters.starred === true ? 1 : 0) + (filters.tag ? 1 : 0));

  function toggle(z: Zone) {
    zone = zone === z ? null : z;
  }
  function onStatus(e: Event) {
    void setStatusFilter((e.currentTarget as HTMLSelectElement).value as StatusFilter);
  }

  const control =
    'inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium dark:border-stone-700';
</script>

{#snippet zoneToggle(id: Zone, label: string, count: number, badge: string)}
  <button
    type="button"
    aria-expanded={zone === id}
    aria-controls={zone === id ? 'filter-pills' : undefined}
    onclick={() => toggle(id)}
    class={`${control} text-stone-600 hover:bg-parchment dark:text-stone-300 dark:hover:bg-stone-800 ${
      zone === id ? 'bg-parchment dark:bg-stone-800' : ''
    }`}
  >
    <ChevronDown size={12} class={`transition-transform ${zone === id ? 'rotate-180' : ''}`} />
    <span>{label}</span>
    {#if zone !== id && count > 0}
      <span class={`rounded-full px-1.5 text-chip tabular-nums ${badge}`}>{count}</span>
    {/if}
  </button>
{/snippet}

<div
  role="group"
  aria-label="Library filters"
  class="shrink-0 border-b border-stone-200 bg-parchment/40 px-4 py-2 dark:border-stone-800 dark:bg-soot/40"
>
  <div class="flex flex-wrap items-center gap-x-2 gap-y-1.5">
    <select
      value={filters.status}
      aria-label="Filter by status"
      onchange={onStatus}
      class="rounded-lg border border-stone-200 bg-paper px-2 py-1 text-xs dark:border-stone-700 dark:bg-stone-800"
    >
      <option value="all">All status</option>
      <option value="resolved">Resolved</option>
      <option value="needs_review">Needs review</option>
    </select>
    {@render zoneToggle('projects', 'Projects', projectCount, 'bg-indigo-600/15 text-indigo-700 dark:text-indigo-300')}
    {@render zoneToggle('tags', 'Star & tags', tagCount, 'bg-amber-700/15 text-amber-800 dark:text-amber-400')}
    {#if relevanceSearchActive()}
      <!-- Batch export filters by the legacy title/author match, not hybrid
           search results — disabled while text/author search terms are active
           to avoid exporting a different set than the table shows. Qualifier-
           only views (tag/project/star/status pills) export the visible set
           exactly, since exportUrl strips q and sends the parsed filters. -->
      <span
        title="Clear the search to export"
        class={`${control} ml-auto cursor-not-allowed text-stone-400 dark:text-stone-600`}
      >
        <Download size={14} /> Export .bib
      </span>
    {:else}
      <a
        href={exportUrl(filters, bibFormat.value)}
        download="xuewen.bib"
        class={`${control} ml-auto text-stone-600 hover:bg-parchment dark:text-stone-300 dark:hover:bg-stone-800`}
      >
        <Download size={14} /> Export .bib
      </a>
    {/if}
  </div>
  {#if zone}
    <div class="mt-2"><FilterPills {zone} /></div>
  {/if}
</div>
