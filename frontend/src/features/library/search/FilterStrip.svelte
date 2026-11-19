<script lang="ts">
  import { ChevronDown, Download } from '@lucide/svelte';
  import { exportShownPapers } from '$features/library/actions';
  import { filters } from './state.svelte';
  import { setStatusFilter } from './actions';
  import FilterPills, { type Zone } from './FilterPills.svelte';
  import type { PaperStatus } from '$shared/api/generated/PaperStatus';

  // One zone at a time, folded by default: the pill rows can grow long, and a
  // count badge on the folded toggle keeps an active filter from hiding.
  let zone = $state<Zone | null>(null);
  const projectCount = $derived(filters.parsed.project !== null ? 1 : 0);
  const tagCount = $derived((filters.starred ? 1 : 0) + (filters.tag ? 1 : 0));

  const STATUS_OPTIONS: { value: PaperStatus | null; label: string }[] = [
    { value: null, label: 'All status' },
    { value: 'resolved', label: 'Resolved' },
    { value: 'needs_review', label: 'Needs review' },
  ];

  function toggle(z: Zone) {
    zone = zone === z ? null : z;
  }
  function onStatus(e: Event & { currentTarget: HTMLSelectElement }) {
    void setStatusFilter(STATUS_OPTIONS[e.currentTarget.selectedIndex].value);
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
      aria-label="Filter by status"
      onchange={onStatus}
      class="rounded-lg border border-stone-200 bg-paper px-2 py-1 text-xs dark:border-stone-700 dark:bg-stone-800"
    >
      {#each STATUS_OPTIONS as option (option.label)}
        <option selected={filters.status === option.value}>{option.label}</option>
      {/each}
    </select>
    {@render zoneToggle('projects', 'Projects', projectCount, 'bg-indigo-600/15 text-indigo-700 dark:text-indigo-300')}
    {@render zoneToggle('tags', 'Star & tags', tagCount, 'bg-amber-700/15 text-amber-800 dark:text-amber-400')}
    <button
      type="button"
      title="Export the papers shown as one .bib file"
      onclick={() => void exportShownPapers()}
      class={`${control} ml-auto text-stone-600 hover:bg-parchment dark:text-stone-300 dark:hover:bg-stone-800`}
    >
      <Download size={14} /> Export .bib
    </button>
  </div>
  {#if zone}
    <div class="mt-2"><FilterPills {zone} /></div>
  {/if}
</div>
