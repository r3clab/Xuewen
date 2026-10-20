<script lang="ts">
  import { Search, SlidersHorizontal } from 'lucide-svelte';
  import { scale } from 'svelte/transition';
  import { clickOutside } from '$shared/ui/clickOutside';
  import { DUR, dur } from '$shared/ui/motion';
  import {
    filters,
    searchMeta,
    searchOpts,
    semanticBlocked,
  } from './state.svelte';
  import {
    setSearch,
    toggleSearchEngine,
    toggleSearchField,
  } from './actions';
  import { revealLibrary } from '$features/reader/tabs/state.svelte';

  let optionsOpen = $state(false);
  const FIELDS = [
    ['title', 'Title'],
    ['authors', 'Authors'],
    ['abstract', 'Abstract'],
    ['body', 'Body'],
    ['notes', 'Notes'],
  ] as const;
  const activeCount = $derived(
    FIELDS.filter(([k]) => searchOpts[k]).length +
      Number(searchOpts.keyword) +
      Number(searchOpts.semantic && !semanticBlocked()),
  );
  // Options the user can actually turn on right now — a server-blocked
  // semantic engine doesn't count, or the "narrowed" hint would never clear.
  const availableCount = $derived(FIELDS.length + 1 + (semanticBlocked() ? 0 : 1));
  const narrowed = $derived(activeCount < availableCount);

  // Only typing navigates: clearing the box or toggling an option over a PDF
  // shouldn't yank the reader away.
  function onInput(e: Event) {
    const value = (e.currentTarget as HTMLInputElement).value;
    setSearch(value);
    if (value.trim() !== '') revealLibrary();
  }

  function onRootKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && optionsOpen) {
      // The popover owns this Esc — it must not also reach the global
      // shortcut handler (which would exit zen or close the palette).
      e.stopPropagation();
      optionsOpen = false;
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -- the div is
     not an interaction target; it only delegates Esc bubbling up from the
     focused input/chips so the popover can close without the global
     shortcut handler also firing. Click-outside is on the whole root (not
     just the popover) so clicking back into the search input keeps it open. -->
<div
  class="relative w-full"
  role="search"
  use:clickOutside={() => {
    if (optionsOpen) optionsOpen = false;
  }}
  onkeydown={onRootKeydown}
>
  <Search size={16} class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-stone-400" />
  <input
    data-search-input
    type="search"
    aria-label="Search papers"
    placeholder={'Search…  tag:nlp author:smith "phrase"'}
    value={filters.q}
    oninput={onInput}
    class={`w-full rounded-lg border border-stone-200 bg-paper py-1.5 pl-9 ${narrowed ? 'pr-24' : 'pr-9'} text-sm outline-none focus:border-amber-700 focus:ring-2 focus:ring-amber-700/15 dark:border-stone-700 dark:bg-stone-800 dark:focus:border-amber-500`}
  />
  <!-- The header has no room for a hint line under the input, so a narrowed
       search says so on the button that undoes it. -->
  <button
    type="button"
    aria-label="Search options"
    title={narrowed ? 'Search options narrowed — click to review' : 'Search options'}
    aria-expanded={optionsOpen}
    onclick={() => (optionsOpen = !optionsOpen)}
    class={`absolute right-2 top-1/2 flex -translate-y-1/2 items-center gap-1 rounded p-1 hover:bg-parchment dark:hover:bg-stone-700 ${
      narrowed
        ? 'text-amber-700 dark:text-amber-500'
        : 'text-stone-400 hover:text-stone-600 dark:hover:text-stone-300'
    }`}
  >
    {#if narrowed}<span class="text-chip font-medium">Narrowed</span>{/if}
    <SlidersHorizontal size={14} />
  </button>

  {#if optionsOpen}
    <div
      transition:scale={{ start: 0.96, duration: dur(DUR.fast) }}
      class="absolute left-0 right-0 top-full z-50 mt-1 min-w-72 space-y-2 rounded-lg border border-stone-200 bg-paper p-2 shadow-lg dark:border-stone-700 dark:bg-soot"
    >
      <p class="text-[10px] font-semibold uppercase tracking-wide text-stone-400">Search in</p>
      <div class="flex flex-wrap gap-1 text-[11px]">
        {#each FIELDS as [key, label] (key)}
          <button
            type="button"
            aria-pressed={searchOpts[key]}
            onclick={() => toggleSearchField(key)}
            class={`rounded-full border px-2 py-0.5 ${
              searchOpts[key]
                ? 'border-amber-700/40 bg-amber-700/10 text-amber-800 dark:border-amber-500/40 dark:bg-amber-500/10 dark:text-amber-400'
                : 'border-stone-200 text-stone-400 dark:border-stone-700 dark:text-stone-500'
            }`}
          >
            {label}
          </button>
        {/each}
      </div>
      <p class="text-[10px] font-semibold uppercase tracking-wide text-stone-400">Engines</p>
      <div class="flex flex-wrap gap-1 text-[11px]">
        <button
          type="button"
          aria-pressed={searchOpts.keyword}
          onclick={() => toggleSearchEngine('keyword')}
          class={`rounded-full border px-2 py-0.5 ${
            searchOpts.keyword
              ? 'border-lime-600/40 bg-lime-600/10 text-lime-800 dark:border-lime-500/40 dark:bg-lime-500/10 dark:text-lime-300'
              : 'border-stone-200 text-stone-400 dark:border-stone-700 dark:text-stone-500'
          }`}
        >
          Keyword
        </button>
        <button
          type="button"
          aria-pressed={searchOpts.semantic && !semanticBlocked()}
          disabled={semanticBlocked()}
          title={searchMeta.semantic.reason ?? undefined}
          onclick={() => toggleSearchEngine('semantic')}
          class={`rounded-full border px-2 py-0.5 disabled:cursor-not-allowed disabled:opacity-40 ${
            searchOpts.semantic && !semanticBlocked()
              ? 'border-lime-600/40 bg-lime-600/10 text-lime-800 dark:border-lime-500/40 dark:bg-lime-500/10 dark:text-lime-300'
              : 'border-stone-200 text-stone-400 dark:border-stone-700 dark:text-stone-500'
          }`}
        >
          Semantic
        </button>
      </div>
      {#if searchMeta.pending > 0}
        <p class="text-[11px] text-stone-400 dark:text-stone-500">
          indexing {searchMeta.pending} paper{searchMeta.pending === 1 ? '' : 's'}…
        </p>
      {/if}
      <p class="text-[10px] text-stone-400 dark:text-stone-500">
        Query syntax: <code>tag:</code> <code>project:</code> <code>is:starred</code>
        <code>status:</code> <code>in:title</code> <code>author:</code> <code>"phrase"</code>
      </p>
    </div>
  {/if}
</div>
