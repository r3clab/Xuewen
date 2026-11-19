import { type FieldKey, FIELD_KEYS, SEMANTIC_FIELD_KEYS, hasSearchTerms, parseQuery } from './query';
import { projects } from '$features/library/organization/state.svelte';
import { FocusTarget } from '$shared/ui/focus';
import type { Engine } from './types';
import type { PaperStatus } from '$shared/api/generated/PaperStatus';
import type { Project } from '$shared/api/generated/Project';
import type { SearchMatch } from '$shared/api/generated/SearchMatch';
import type { SemanticAvailability } from '$shared/api/generated/SemanticAvailability';
import type { Sort } from '$shared/api/generated/Sort';

/// The library view's query. The search box string is the single source of
/// truth for every filter — pills, the status control and the field toggles
/// all edit it — so everything else here is read off its parse.
class LibraryQuery {
  q = $state('');
  /// A query parameter rather than a qualifier; relevance search ignores it.
  sort = $state<Sort>('year_desc');

  readonly parsed = $derived(parseQuery(this.q));
  /// Free text or author terms: the query goes through the search engines
  /// (relevance-ranked, with snippets) rather than only filtering the list.
  readonly searching = $derived(hasSearchTerms(this.parsed));
  /// The project `project:` names, matched case-insensitively as the server
  /// does; null when absent or when the name matches no project.
  readonly project = $derived.by((): Project | null => {
    const name = this.parsed.project?.toLowerCase();
    return name === undefined ? null : (projects.items.find((p) => p.name.toLowerCase() === name) ?? null);
  });
  /// The fields the query searches: every one unless `in:` narrows it.
  readonly fields = $derived<readonly FieldKey[]>(this.parsed.fields ?? FIELD_KEYS);

  get tag(): string | null {
    return this.parsed.tag;
  }
  get starred(): boolean {
    return this.parsed.starred;
  }
  get status(): PaperStatus | null {
    return this.parsed.status;
  }
}

export const filters = new LibraryQuery();

/// Which search engines run; at least one is always on.
export const engines = $state<Record<Engine, boolean>>({ keyword: true, semantic: true });

/// The search box, for the keymap's `/`.
export const searchFocus = new FocusTarget();

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
  const noEmbedded = !SEMANTIC_FIELD_KEYS.some((f) => filters.fields.includes(f));
  return noEmbedded || !searchMeta.semantic.available;
}

/// Whether any list filter deviates from the default view — i.e. whether an
/// empty list means "nothing matches" rather than "the library is empty".
export function anyFilterActive(): boolean {
  return filters.q.trim() !== '';
}

/// Human-readable names for every active filter — what an empty state should
/// blame ("No papers match X · Y").
function activeFilterLabels(): string[] {
  const p = filters.parsed;
  const labels: string[] = [];
  if (p.text.trim()) labels.push(`“${p.text.trim()}”`);
  for (const author of p.authors) labels.push(`author ${author}`);
  if (p.project) labels.push(filters.project?.name ?? p.project);
  if (p.tag) labels.push(p.tag);
  if (p.starred) labels.push('starred');
  if (p.status) labels.push(p.status === 'needs_review' ? 'needs review' : p.status);
  return labels;
}

/// The filtered-empty sentence the Welcome hero shows above its Clear-filters
/// button.
export function noMatchesBlame(): string {
  return `No papers match ${activeFilterLabels().join(' · ')}.`;
}
