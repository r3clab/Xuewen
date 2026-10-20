import {
  type ParsedQuery,
  FIELD_KEYS,
  SEMANTIC_FIELD_KEYS,
  hasSearchTerms,
  parseQuery,
} from './query';
import type { Filters, SearchOpts } from './types';
import type { Project } from '$shared/api/generated/Project';
import type { SearchMatch } from '$shared/api/generated/SearchMatch';
import type { SemanticAvailability } from '$shared/api/generated/SemanticAvailability';

export const filters = $state<Filters>({
  q: '',
  status: 'all',
  sort: 'year_desc',
  project: 'all',
  tag: undefined,
  starred: undefined,
});

/// The search box string is the single source of truth; project/tag/starred/
/// status on `filters` (and the field toggles on `searchOpts`) are a cache of
/// its parse — `project:` names resolve to ids via the loaded projects list
/// (unresolved names pass through verbatim and simply match nothing).
export function syncFiltersFromQuery(projects: Project[]): ParsedQuery {
  const p = parseQuery(filters.q);
  filters.tag = p.tag ?? undefined;
  filters.starred = p.starred || undefined;
  filters.status = p.status ?? 'all';
  filters.project = p.project
    ? (projects.find((pr) => pr.name.toLowerCase() === p.project!.toLowerCase())?.id ?? p.project)
    : 'all';
  for (const k of FIELD_KEYS) {
    searchOpts[k] = p.fields === null || p.fields.includes(k);
  }
  return p;
}

export const searchOpts = $state<SearchOpts>({
  title: true,
  authors: true,
  abstract: true,
  body: true,
  notes: true,
  keyword: true,
  semantic: true,
});

/// Match info per paper id for the current search, plus the semantic tier's
/// availability (from the last response or /api/search/status).
export const searchMeta = $state<{
  byId: Record<string, SearchMatch>;
  semantic: SemanticAvailability;
  /// Papers still waiting for a tier to index (drives "indexing N papers…").
  pending: number;
}>({ byId: {}, semantic: { available: true, reason: null }, pending: 0 });

export const finder = $state<{ open: boolean }>({ open: false });

/// Semantic chip is disabled when the backend can't serve it or the field
/// selection makes it meaningless (nothing embedded is selected).
export function semanticBlocked(): boolean {
  const noEmbedded = !SEMANTIC_FIELD_KEYS.some((f) => searchOpts[f]);
  return noEmbedded || !searchMeta.semantic.available;
}

/// Whether the query goes through the search engines (free text or author
/// terms) rather than only filtering the list: results are then relevance-
/// ranked, sort is ignored, and match snippets exist.
export function relevanceSearchActive(): boolean {
  return hasSearchTerms(parseQuery(filters.q));
}

/// Whether any list filter deviates from the default view — i.e. whether an
/// empty list means "nothing matches" rather than "the library is empty".
export function anyFilterActive(): boolean {
  return (
    filters.q.trim() !== '' ||
    filters.status !== 'all' ||
    filters.project !== 'all' ||
    filters.tag !== undefined ||
    filters.starred !== undefined
  );
}

/// Human-readable names for every non-default list filter — what an empty
/// state should blame ("No papers match X · Y").
function activeFilterLabels(projects: Project[]): string[] {
  const labels: string[] = [];
  if (filters.q.trim()) labels.push(`“${filters.q.trim()}”`);
  if (filters.project !== 'all')
    labels.push(projects.find((p) => p.id === filters.project)?.name ?? 'the selected project');
  if (filters.tag) labels.push(filters.tag);
  if (filters.starred !== undefined) labels.push('starred');
  if (filters.status !== 'all')
    labels.push(filters.status === 'needs_review' ? 'needs review' : filters.status);
  return labels;
}

/// The filtered-empty sentence the Welcome hero shows above its Clear-filters
/// button.
export function noMatchesBlame(projects: Project[]): string {
  return `No papers match ${activeFilterLabels(projects).join(' · ')}.`;
}
