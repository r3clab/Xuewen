import { SvelteMap } from 'svelte/reactivity';
import type { BibFormat } from './types';
import type { PaperDetail } from '$shared/api/generated/PaperDetail';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';
import type { Stats } from '$shared/api/generated/Stats';

export const library = $state<{
  papers: PaperSummary[];
  loading: boolean;
  /// False until the first load settles: before that an empty list means
  /// "not fetched yet", not "the library is empty".
  loaded: boolean;
  error: string | null;
  revision: number;
}>({ papers: [], loading: false, loaded: false, error: null, revision: 0 });

export const stats = $state<{ value: Stats | null }>({ value: null });

export const bibFormat = $state<{ value: BibFormat }>({ value: 'bibtex' });

export function markLibraryChanged(): void {
  library.revision++;
}

/// Cached full record per paper id. Reactive (SvelteMap), so an open view
/// (DockDetails) re-renders in place when a mutation patches a record and
/// refetches when one is evicted — no remount counter, no double bookkeeping.
/// Reactivity is key-level only: writes must go through `set` with a NEW
/// object (what `patchDetail` does); mutating a stored object in place would
/// go unnoticed.
const details = new SvelteMap<string, PaperDetail>();

export function cachedDetail(id: string): PaperDetail | undefined {
  return details.get(id);
}

/// Merge a partial update into a cached detail. A cache miss is a no-op —
/// the next `loadDetail` fetches fresh anyway.
export function patchDetail(id: string, patch: Partial<PaperDetail>): void {
  const cur = details.get(id);
  if (cur) details.set(id, { ...cur, ...patch });
}

/// Replace a cached record outright (identify's authoritative server echo).
export function setDetail(id: string, d: PaperDetail): void {
  details.set(id, d);
}

export function evictDetail(id: string): void {
  details.delete(id);
}

/// A global tag/project rename or delete stales the names embedded in every
/// cached record: evict everything and let open views refetch on miss.
export function clearDetails(): void {
  details.clear();
}

export function applyPaperName(id: string, name: string | null): void {
  const row = library.papers.find((p) => p.id === id);
  if (row) row.name = name;
  patchDetail(id, { name });
}

export function dropPaper(id: string): void {
  library.papers = library.papers.filter((p) => p.id !== id);
  evictDetail(id);
}
