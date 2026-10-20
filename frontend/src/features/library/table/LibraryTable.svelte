<script lang="ts">
  import { ArrowDown, ArrowUp, Bookmark, Star, Tag, Trash2, X } from 'lucide-svelte';
  import ColumnHeaderMenu from './ColumnHeaderMenu.svelte';
  import ColumnResizeHandle from './ColumnResizeHandle.svelte';
  import ConfirmButtons from '$shared/ui/ConfirmButtons.svelte';
  import PaperRowTags from './PaperRowTags.svelte';
  import StatusPill from '$features/library/StatusPill.svelte';
  import { openContextMenu } from './contextMenu.svelte';
  import { removePapers } from '$app/actions';
  import { toggleStar } from '$features/library/actions';
  import { library } from '$features/library/state.svelte';
  import { addPapersToProject, addTagToPapers } from '$features/library/organization/actions';
  import { projects } from '$features/library/organization/state.svelte';
  import { setSortFilter } from '$features/library/search/actions';
  import {
    filters,
    relevanceSearchActive,
    searchMeta,
  } from '$features/library/search/state.svelte';
  import { openTab, selection, selectPaper } from '$features/reader/tabs/state.svelte';
  import { toast } from '$shared/ui/toasts.svelte';
  import { abbreviateVenue } from '$features/library/venue';
  import { fitToAvailable, measureColumnFromDom } from './columnAutoFit';
  import {
    columnWidths,
    commitColumnWidth,
    commitColumnWidths,
    resetColumnWidths,
    setColumnWidth,
  } from './columnWidths.svelte';
  import {
    AUTO_FIT_PADDING,
    ICON_COLUMN_PX,
    PINNED_COLUMNS,
    PINNED_KEYS,
    autoFitBudget,
    dragCeiling,
    tableMinWidth,
    type PinnedColumnKey,
  } from './columns';
  import type { PaperSummary } from '$shared/api/generated/PaperSummary';
  import type { Sort } from '$features/library/search/types';

  // Multi-select for bulk actions. Lives here (not in global state): it only
  // means anything while the table is on screen, and pruning keeps it honest
  // when a filter change or delete drops papers out of the list.
  let selected = $state<string[]>([]);
  let confirmingDelete = $state(false);
  let tagDraft = $state('');
  let busy = $state(false);

  // Memoized id sets: the prune effect re-runs on every selection change too,
  // and each row's checkbox re-checks membership when `selected` changes —
  // both stay O(1)-per-lookup instead of rescanning arrays.
  const paperIds = $derived(new Set(library.papers.map((p) => p.id)));
  const selectedSet = $derived(new Set(selected));

  $effect(() => {
    if (selected.some((id) => !paperIds.has(id))) {
      selected = selected.filter((id) => paperIds.has(id));
    }
  });

  const allSelected = $derived(
    library.papers.length > 0 && selected.length === library.papers.length,
  );

  function toggleOne(id: string) {
    selected = selectedSet.has(id) ? selected.filter((x) => x !== id) : [...selected, id];
  }
  function toggleAll() {
    selected = allSelected ? [] : library.papers.map((p) => p.id);
  }
  function clearSelection() {
    selected = [];
    confirmingDelete = false;
  }

  function setSort(s: Sort) {
    void setSortFilter(s);
  }

  // While search terms are active the server ranks by relevance; sort headers
  // would lie, so they go inert (arrows off, buttons disabled, no aria-sort).
  // Qualifier-only queries (pills) still list in sort order.
  const searching = $derived(relevanceSearchActive());

  // One sort per header for Name/Title/Added; Year toggles between both
  // directions. The sortHeader snippet derives aria-sort, the arrow, and the
  // click target from this one spec, so the direction logic exists once.
  type SortSpec = { label: string; sort: Sort } | { label: string; asc: Sort; desc: Sort };
  function directionOf(spec: SortSpec, current: Sort): 'ascending' | 'descending' | undefined {
    const variants = 'sort' in spec ? [spec.sort] : [spec.asc, spec.desc];
    if (!variants.includes(current)) return undefined;
    return current.endsWith('_desc') ? 'descending' : 'ascending';
  }
  function nextSortOf(spec: SortSpec): Sort {
    if ('sort' in spec) return spec.sort;
    return filters.sort === spec.desc ? spec.asc : spec.desc;
  }

  let tableEl = $state<HTMLTableElement | null>(null);
  let paneEl = $state<HTMLDivElement | null>(null);
  let headerMenu = $state<{ x: number; y: number } | null>(null);

  // Under `table-layout: fixed` the width-less Tags column only ever gets the
  // remainder — and once the pinned widths alone exceed the pane that
  // remainder is *zero*. Tags then wrapped one chip per line, so the papers
  // with the most tags stood several rows tall next to their neighbours. The
  // floor turns that into the honest outcome instead: the pane scrolls
  // sideways and every column keeps a width you can read.
  const minTableWidth = $derived(tableMinWidth(columnWidths));

  // Measured against the *pane*, not the table: the table can no longer be
  // narrower than minTableWidth, so sizing to it would let auto-fit grow
  // columns into surplus that only exists because the columns are already
  // too wide.
  function maxFor(key: PinnedColumnKey): number {
    return dragCeiling(key, columnWidths, paneEl?.clientWidth ?? 0);
  }

  function autoFit(key: PinnedColumnKey) {
    if (!tableEl) return;
    const def = PINNED_COLUMNS[key];
    commitColumnWidth(
      key,
      measureColumnFromDom(tableEl, key, {
        min: def.minWidth,
        max: maxFor(key),
        padding: AUTO_FIT_PADDING,
      }),
    );
  }

  // Fit every column to its content, then scale the whole set toward the
  // pane width — down when it doesn't fit, up into surplus when it does, so
  // spare width lands in the content columns rather than pooling in Tags
  // (which keeps a comfortable TAGS_TARGET_PX strip).
  function autoFitAll() {
    if (!tableEl) return;
    const natural = {} as Record<PinnedColumnKey, number>;
    const bounds = {} as Record<PinnedColumnKey, { min: number; max: number }>;
    for (const key of PINNED_KEYS) {
      const def = PINNED_COLUMNS[key];
      natural[key] = measureColumnFromDom(tableEl, key, {
        min: def.minWidth,
        max: def.maxWidth,
        padding: AUTO_FIT_PADDING,
      });
      bounds[key] = { min: def.minWidth, max: def.maxWidth };
    }
    const container = paneEl?.clientWidth ?? 0;
    const fitted =
      container > 0 ? fitToAvailable(natural, bounds, autoFitBudget(container)) : natural;
    commitColumnWidths(fitted);
  }

  function onHeaderContextMenu(e: MouseEvent) {
    e.preventDefault();
    headerMenu = { x: e.clientX, y: e.clientY };
  }

  // One shared formatter: constructing Intl.DateTimeFormat per row per render
  // (what toLocaleDateString with options does) is measurably expensive.
  const DATE_FMT = new Intl.DateTimeFormat(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });

  function open(p: PaperSummary) {
    openTab(p);
  }
  function onRowContextMenu(e: MouseEvent, p: PaperSummary) {
    selectPaper(p.id);
    openContextMenu(e, p);
  }

  // One busy flag and one error surface for every bulk action — without the
  // catch, a failed request is an unhandled rejection with zero UI feedback
  // (the single-paper delete paths all catch and toast; bulk must too).
  async function run(fn: () => Promise<void>) {
    busy = true;
    try {
      await fn();
    } catch (e) {
      toast('error', (e as Error).message);
    } finally {
      busy = false;
    }
  }
  function bulkStar() {
    const targets = library.papers.filter((p) => selectedSet.has(p.id) && !p.starred);
    void run(async () => {
      // Independent papers; toggleStar handles its own rollback/toast per id.
      await Promise.all(targets.map((p) => toggleStar(p.id)));
    });
  }
  function bulkTag() {
    const name = tagDraft.trim();
    if (!name) return;
    const ids = [...selected];
    void run(async () => {
      await addTagToPapers(ids, name);
      tagDraft = '';
    });
  }
  function bulkProject(e: Event) {
    const sel = e.currentTarget as HTMLSelectElement;
    const projectId = sel.value;
    sel.value = '';
    if (!projectId) return;
    const ids = [...selected];
    void run(() => addPapersToProject(ids, projectId));
  }
  function bulkDelete() {
    const ids = [...selected];
    clearSelection();
    void run(() => removePapers(ids));
  }

  const th =
    'px-3 py-1.5 text-left text-caption font-semibold uppercase tracking-[.07em] text-stone-500 dark:text-stone-400';
  const sortBtn =
    'inline-flex items-center gap-1 rounded uppercase tracking-[.07em] hover:text-ink disabled:cursor-default disabled:hover:text-inherit dark:hover:text-stone-200';
  const td = 'px-3 py-1.5 align-top';
  const bulkBtn =
    'inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-stone-600 hover:bg-parchment disabled:opacity-50 dark:border-stone-700 dark:text-stone-300 dark:hover:bg-stone-800';
</script>

<div class="flex min-h-0 min-w-0 flex-1 flex-col">
  {#if selected.length > 0}
    <div class="flex shrink-0 flex-wrap items-center gap-2 border-b border-stone-200 bg-parchment/60 px-4 py-2 dark:border-stone-800 dark:bg-stone-800/40">
      <span class="text-xs font-medium text-stone-600 dark:text-stone-300">{selected.length} selected</span>
      <button type="button" class={bulkBtn} disabled={busy} onclick={bulkStar}>
        <Star size={13} /> Star
      </button>
      <form
        class="flex items-center gap-1"
        onsubmit={(e) => {
          e.preventDefault();
          bulkTag();
        }}
      >
        <Tag size={13} class="text-stone-400" />
        <input
          bind:value={tagDraft}
          placeholder="Add tag…"
          class="w-28 rounded-lg border border-stone-200 bg-paper px-2 py-1 text-xs outline-none focus:border-amber-700 dark:border-stone-700 dark:bg-stone-800 dark:focus:border-amber-500"
        />
        <button type="submit" aria-label="Apply tag" class={bulkBtn} disabled={busy || !tagDraft.trim()}>Apply tag</button>
      </form>
      <label class="flex items-center gap-1 text-xs text-stone-500 dark:text-stone-400">
        <Bookmark size={13} class="text-stone-400" />
        <select
          aria-label="Add to project"
          onchange={bulkProject}
          disabled={busy}
          class="rounded-lg border border-stone-200 bg-paper px-2 py-1 text-xs dark:border-stone-700 dark:bg-stone-800"
        >
          <option value="">Add to project…</option>
          {#each projects.items as pr (pr.id)}
            <option value={pr.id}>{pr.name}</option>
          {/each}
        </select>
      </label>
      <span class="min-w-0 flex-1"></span>
      {#if confirmingDelete}
        <ConfirmButtons
          confirmLabel={`Delete ${selected.length}`}
          onConfirm={bulkDelete}
          onCancel={() => (confirmingDelete = false)}
        />
      {:else}
        <button
          type="button"
          class="inline-flex items-center gap-1.5 rounded-lg border border-stone-200 px-2 py-1 text-xs font-medium text-red-600 hover:bg-red-600/10 disabled:opacity-50 dark:border-stone-700 dark:text-red-400"
          disabled={busy}
          onclick={() => (confirmingDelete = true)}
        >
          <Trash2 size={13} /> Delete
        </button>
      {/if}
      <button type="button" aria-label="Clear selection" class={bulkBtn} onclick={clearSelection}>
        <X size={13} />
      </button>
    </div>
  {/if}

  {#snippet resizeHandle(key: PinnedColumnKey, edge: 'left' | 'right' = 'right')}
    <ColumnResizeHandle
      label={PINNED_COLUMNS[key].label}
      width={columnWidths[key]}
      min={PINNED_COLUMNS[key].minWidth}
      max={() => maxFor(key)}
      {edge}
      onLiveResize={(px) => setColumnWidth(key, px)}
      onCommit={(px) => commitColumnWidth(key, px)}
      onAutoFit={() => autoFit(key)}
    />
  {/snippet}

  {#snippet sortHeader(key: PinnedColumnKey, spec: SortSpec, edge: 'left' | 'right' = 'right')}
    {@const dir = searching ? undefined : directionOf(spec, filters.sort)}
    <th class={`${th} relative`} data-col={key} aria-sort={dir}>
      <button
        type="button"
        class={sortBtn}
        disabled={searching}
        title={searching ? 'Sorted by relevance during search' : undefined}
        onclick={() => setSort(nextSortOf(spec))}
      >
        {spec.label}{#if dir === 'descending'}<ArrowDown size={11} />{:else if dir === 'ascending'}<ArrowUp size={11} />{/if}
      </button>
      {@render resizeHandle(key, edge)}
    </th>
  {/snippet}

  <div bind:this={paneEl} class="min-h-0 flex-1 overflow-auto">
    <table
      bind:this={tableEl}
      style={`min-width:${minTableWidth}px`}
      class="w-full table-fixed border-collapse text-sm"
    >
      <!-- Single source of column widths. Tags has no <col> width on
           purpose: under table-fixed the one width-less column absorbs the
           remainder, which is what keeps the table exactly pane-wide. -->
      <colgroup>
        <col style={`width:${ICON_COLUMN_PX}px`} />
        <col style={`width:${ICON_COLUMN_PX}px`} />
        <col style={`width:${columnWidths.name}px`} />
        <col style={`width:${columnWidths.title}px`} />
        <col style={`width:${columnWidths.firstAuthor}px`} />
        <col style={`width:${columnWidths.lastAuthor}px`} />
        <col style={`width:${columnWidths.venue}px`} />
        <col style={`width:${columnWidths.year}px`} />
        <col />
        <col style={`width:${columnWidths.added}px`} />
      </colgroup>
      <thead class="sticky top-0 z-10 bg-paper dark:bg-night">
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -- the
             right-click column menu is a mouse convenience; resizing stays
             keyboard-reachable through the focusable handles. -->
        <tr
          class="border-b border-stone-200 dark:border-stone-800"
          oncontextmenu={onHeaderContextMenu}
        >
          <th class="px-3 py-1.5">
            <input
              type="checkbox"
              aria-label="Select all"
              checked={allSelected}
              onchange={toggleAll}
              class="accent-amber-700"
            />
          </th>
          <th></th>
          {@render sortHeader('name', { label: 'Name', sort: 'name' })}
          {@render sortHeader('title', { label: 'Title', sort: 'title' })}
          <th class={`${th} relative`} data-col="firstAuthor">
            First author
            {@render resizeHandle('firstAuthor')}
          </th>
          <th class={`${th} relative`} data-col="lastAuthor">
            Last author
            {@render resizeHandle('lastAuthor')}
          </th>
          <th class={`${th} relative`} data-col="venue">
            Venue
            {@render resizeHandle('venue')}
          </th>
          {@render sortHeader('year', { label: 'Year', asc: 'year_asc', desc: 'year_desc' })}
          <th class={th}>Tags</th>
          <!-- Added's handle is left-edge: its right edge IS the table edge;
               the divider it owns is the Tags|Added boundary on its left. -->
          {@render sortHeader('added', { label: 'Added', sort: 'added_desc' }, 'left')}
        </tr>
      </thead>
      <tbody>
        {#each library.papers as p (p.id)}
          <!-- svelte-ignore a11y_no_noninteractive_element_interactions -- the
               row's click/contextmenu are mouse conveniences; the accessible
               controls are the nested title button, star button, and checkbox. -->
          <tr
            data-cursor={selection.id === p.id ? 'true' : undefined}
            onclick={() => open(p)}
            oncontextmenu={(e) => onRowContextMenu(e, p)}
            class={`cursor-pointer border-b border-stone-200/60 transition-colors hover:bg-parchment/70 dark:border-stone-800/60 dark:hover:bg-stone-800/40 ${
              selection.id === p.id ? 'bg-parchment dark:bg-stone-800/50' : ''
            }`}
          >
            <td class={td} onclick={(e) => e.stopPropagation()}>
              <input
                type="checkbox"
                aria-label={`Select ${p.title ?? p.id}`}
                checked={selectedSet.has(p.id)}
                onchange={() => toggleOne(p.id)}
                class="accent-amber-700"
              />
            </td>
            <td class={td}>
              <button
                type="button"
                aria-label={p.starred ? 'Unstar paper' : 'Star paper'}
                aria-pressed={p.starred}
                onclick={(e) => {
                  e.stopPropagation();
                  void toggleStar(p.id);
                }}
                class={p.starred
                  ? 'text-orange-500'
                  : 'text-stone-300 hover:text-orange-400 dark:text-stone-600'}
              >
                <Star size={14} fill={p.starred ? 'currentColor' : 'none'} />
              </button>
            </td>
            <!-- Semibold at full ink, not the muted stone the other metadata
                 columns wear: the name is a handle you scan for. No amber
                 here, unlike the Details dock chip — its own labelled column
                 already says what it is, and a whole column of the app's
                 action color would drown the selected row. -->
            <td class={`${td} font-sans text-xs font-semibold text-ink dark:text-stone-100`}>
              <div class="truncate" data-col="name" title={p.name ?? undefined}>
                <!-- font-normal on the dash only: the ghost em-dash is the
                     table's shared empty-state idiom and shouldn't come out
                     heavier here than in every other column. -->
                {#if p.name}{p.name}{:else}<span class="font-normal text-stone-300 dark:text-stone-600">—</span>{/if}
              </div>
            </td>
            <td class={td}>
              <button
                type="button"
                data-col="title"
                onclick={(e) => {
                  e.stopPropagation();
                  open(p);
                }}
                class="text-left font-serif font-medium text-ink hover:underline dark:text-stone-100"
              >
                {p.title ?? '(untitled)'}
              </button>
              <StatusPill status={p.status} class="ml-1.5 align-middle" />
              <!-- Gated on `searching` too: byId outlives a cleared query until
                   the reload lands, and a stale snippet would explain nothing. -->
              {#if searching && searchMeta.byId[p.id]}
                {@const m = searchMeta.byId[p.id]}
                <div class="mt-0.5 line-clamp-2 break-words text-xs text-stone-600 dark:text-stone-300">
                  <span class="mr-1 rounded bg-parchment px-1 py-px font-mono text-chip uppercase tracking-wide text-stone-500 dark:bg-stone-800 dark:text-stone-400">
                    {m.field}{#if m.page != null}&nbsp;p.{m.page}{/if}
                  </span>
                  <!-- SAFETY: the server HTML-escapes snippet text and emits only <mark> tags. -->
                  <span class="[&_mark]:rounded [&_mark]:bg-yellow-200 [&_mark]:px-0.5 dark:[&_mark]:bg-yellow-500/40">
                    {@html m.snippet}
                  </span>
                </div>
              {/if}
            </td>
            <!-- First/last author are the ordered list's ends; a single-
                 author paper repeats the name in both columns ([0] and
                 .at(-1) coincide). Tooltips keep the full roster — the only
                 place elided middle authors still show. -->
            {#snippet authorCell(col: 'firstAuthor' | 'lastAuthor')}
              {@const name = col === 'firstAuthor' ? p.authors[0] : p.authors.at(-1)}
              <td class={`${td} text-stone-500 dark:text-stone-400`}>
                <div class="truncate" data-col={col} title={p.authors.join(', ') || undefined}>
                  {#if name}{name}{:else}<span class="text-stone-300 dark:text-stone-600">—</span>{/if}
                </div>
              </td>
            {/snippet}
            {@render authorCell('firstAuthor')}
            {@render authorCell('lastAuthor')}
            <td class={`${td} text-stone-500 dark:text-stone-400`}>
              <div class="truncate" data-col="venue" title={p.venue ?? undefined}>
                {#if p.venue}{abbreviateVenue(p.venue)}{:else}<span class="text-stone-300 dark:text-stone-600">—</span>{/if}
              </div>
            </td>
            <td class={`${td} tabular-nums text-stone-500 dark:text-stone-400`} data-col="year">
              {#if p.year !== null}{p.year}{:else}<span class="text-stone-300 dark:text-stone-600">—</span>{/if}
            </td>
            <td class={td}><PaperRowTags paper={p} /></td>
            <td class={`${td} whitespace-nowrap text-stone-400 dark:text-stone-500`} data-col="added">
              {p.added_at ? DATE_FMT.format(new Date(p.added_at)) : ''}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>

  {#if headerMenu}
    <ColumnHeaderMenu
      x={headerMenu.x}
      y={headerMenu.y}
      onAutoFitAll={autoFitAll}
      onReset={resetColumnWidths}
      onClose={() => (headerMenu = null)}
    />
  {/if}
</div>
