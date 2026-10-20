import { getSearchStatus } from './api';
import {
  type FieldKey,
  FIELD_KEYS,
  hasSearchTerms,
  setFieldQualifiers,
  setQualifier,
  setStarredQualifier,
} from './query';
import { filters, searchMeta, searchOpts, semanticBlocked, syncFiltersFromQuery } from './state.svelte';
import { loadPapers } from '$features/library/actions';
import { projects } from '$features/library/organization/state.svelte';
import type { Sort, StatusFilter } from './types';

export function toggleSearchField(k: FieldKey): void {
  const on = FIELD_KEYS.filter((f) => searchOpts[f]);
  if (searchOpts[k] && on.length === 1) return; // keep at least one field
  const next = searchOpts[k] ? on.filter((f) => f !== k) : [...on, k];
  filters.q = setFieldQualifiers(filters.q, next.length === FIELD_KEYS.length ? null : next);
  if (hasSearchTerms(syncFiltersFromQuery(projects.items))) void loadPapers();
}

export function toggleSearchEngine(k: 'keyword' | 'semantic'): void {
  const other = k === 'keyword' ? 'semantic' : 'keyword';
  if (searchOpts[k] && !searchOpts[other]) return; // keep at least one engine
  searchOpts[k] = !searchOpts[k];
  if (filters.q.trim()) void loadPapers();
}

export async function loadSearchStatus(): Promise<void> {
  try {
    const st = await getSearchStatus();
    searchMeta.semantic = { available: st.semantic_available, reason: st.reason };
    searchMeta.pending = Math.max(st.fts.pending, st.vectors.pending);
  } catch (e) {
    console.error(e); // e.g. 503 search not configured -> leave defaults
  }
}

/// Pill clicks and the status control edit the query string; the parse then
/// round-trips into the cached filters. Filters combine (AND) — the old
/// project/tag/star mutual exclusivity is gone by design. Also the write path
/// for tag/project rename+delete, which rewrite the qualifier they own.
export async function applyQueryEdit(q: string): Promise<void> {
  clearTimeout(kwDebounce);
  clearTimeout(fullDebounce);
  filters.q = q;
  syncFiltersFromQuery(projects.items);
  await loadPapers();
}

export async function setProjectFilter(id: string): Promise<void> {
  const name = id === 'all' ? null : (projects.items.find((p) => p.id === id)?.name ?? id);
  await applyQueryEdit(setQualifier(filters.q, 'project', name));
}

export async function setTagFilter(tag: string | undefined): Promise<void> {
  await applyQueryEdit(setQualifier(filters.q, 'tag', tag ?? null));
}

export async function setStarFilter(on: boolean): Promise<void> {
  await applyQueryEdit(setStarredQualifier(filters.q, on));
}

export async function setStatusFilter(status: StatusFilter): Promise<void> {
  await applyQueryEdit(setQualifier(filters.q, 'status', status === 'all' ? null : status));
}

/// Sort lives on `filters` directly (it's a query param, not a search-string
/// qualifier), but changing it still means "mutate + reload" like the setters
/// above — the table's sort headers come here.
export async function setSortFilter(sort: Sort): Promise<void> {
  filters.sort = sort;
  await loadPapers();
}

/// Reset every list filter (search, status, project/tag/star) to the default
/// view and reload — the escape hatch offered by the list's empty state.
export async function clearFilters(): Promise<void> {
  filters.q = '';
  syncFiltersFromQuery(projects.items); // empty q → all filters default, all fields on
  await loadPapers();
}

let kwDebounce: ReturnType<typeof setTimeout> | undefined;
let fullDebounce: ReturnType<typeof setTimeout> | undefined;
export function setSearch(q: string): void {
  filters.q = q;
  const parsed = syncFiltersFromQuery(projects.items);
  clearTimeout(kwDebounce);
  clearTimeout(fullDebounce);
  if (!hasSearchTerms(parsed)) {
    // Qualifier-only (or empty) → plain list; small debounce so typing a
    // qualifier character-by-character doesn't fire a request per keystroke.
    kwDebounce = setTimeout(() => void loadPapers(), 150);
    return;
  }
  // Fast keyword-only pass while typing; the full (semantic) pass once settled.
  if (searchOpts.keyword) {
    kwDebounce = setTimeout(() => void loadPapers({ keywordOnly: true }), 150);
  }
  if (searchOpts.semantic && !semanticBlocked()) {
    fullDebounce = setTimeout(() => void loadPapers(), 600);
  } else if (!searchOpts.keyword) {
    fullDebounce = setTimeout(() => void loadPapers(), 600);
  }
}
