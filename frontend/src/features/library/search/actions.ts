import { getSearchStatus } from './api';
import { type FieldKey, FIELD_KEYS, setFieldQualifiers, setQualifier, setStarredQualifier } from './query';
import { engines, filters, searchMeta, semanticBlocked } from './state.svelte';
import { loadPapers } from '$features/library/actions';
import type { Engine } from './types';
import type { PaperStatus } from '$shared/api/generated/PaperStatus';
import type { Sort } from '$shared/api/generated/Sort';

export function toggleSearchField(k: FieldKey): void {
  const on = filters.fields;
  if (on.includes(k) && on.length === 1) return; // keep at least one field
  const next = on.includes(k) ? on.filter((f) => f !== k) : [...on, k];
  filters.q = setFieldQualifiers(filters.q, next.length === FIELD_KEYS.length ? null : next);
  if (filters.searching) void loadPapers();
}

export function toggleSearchEngine(k: Engine): void {
  const other = k === 'keyword' ? 'semantic' : 'keyword';
  if (engines[k] && !engines[other]) return; // keep at least one engine
  engines[k] = !engines[k];
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

/// Pill clicks and the status control edit the query string, and the list
/// reloads for it. Filters combine (AND). Also the write path for tag/project
/// rename+delete, which rewrite the qualifier they own.
export async function applyQueryEdit(q: string): Promise<void> {
  clearTimeout(kwDebounce);
  clearTimeout(fullDebounce);
  filters.q = q;
  await loadPapers();
}

export async function setProjectFilter(name: string | null): Promise<void> {
  await applyQueryEdit(setQualifier(filters.q, 'project', name));
}

export async function setTagFilter(tag: string | null): Promise<void> {
  await applyQueryEdit(setQualifier(filters.q, 'tag', tag));
}

export async function setStarFilter(on: boolean): Promise<void> {
  await applyQueryEdit(setStarredQualifier(filters.q, on));
}

export async function setStatusFilter(status: PaperStatus | null): Promise<void> {
  await applyQueryEdit(setQualifier(filters.q, 'status', status));
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
  await applyQueryEdit('');
}

let kwDebounce: ReturnType<typeof setTimeout> | undefined;
let fullDebounce: ReturnType<typeof setTimeout> | undefined;
export function setSearch(q: string): void {
  filters.q = q;
  clearTimeout(kwDebounce);
  clearTimeout(fullDebounce);
  if (!filters.searching) {
    // Qualifier-only (or empty) → plain list; small debounce so typing a
    // qualifier character-by-character doesn't fire a request per keystroke.
    kwDebounce = setTimeout(() => void loadPapers(), 150);
    return;
  }
  // Fast keyword-only pass while typing; the full (semantic) pass once settled.
  if (engines.keyword) {
    kwDebounce = setTimeout(() => void loadPapers({ keywordOnly: true }), 150);
  }
  if (engines.semantic && !semanticBlocked()) {
    fullDebounce = setTimeout(() => void loadPapers(), 600);
  } else if (!engines.keyword) {
    fullDebounce = setTimeout(() => void loadPapers(), 600);
  }
}
