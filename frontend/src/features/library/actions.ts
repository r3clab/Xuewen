import { exportPapers, getPaper, getStats, listPapers, setStar } from './api';
import { searchPapers } from '$features/library/search/api';
import { engines, filters, searchMeta } from '$features/library/search/state.svelte';
import { bibFormat, cachedDetail, library, patchDetail, setDetail, stats } from './state.svelte';
import { downloadBlob } from '$shared/browser/download';
import { errorMessage } from '$shared/errors';
import { toast } from '$shared/ui/toasts.svelte';
import type { PaperDetail } from '$shared/api/generated/PaperDetail';

export async function loadStats(): Promise<void> {
  try {
    stats.value = await getStats();
  } catch (e) {
    console.error(e);
  }
}

let seq = 0;
export async function loadPapers(opts?: { keywordOnly?: boolean }): Promise<void> {
  const my = ++seq;
  library.loading = true;
  library.error = null;
  try {
    if (!filters.searching) {
      // Qualifier-only (or empty) query: the plain filtered, sorted list.
      const papers = await listPapers({ q: filters.q, sort: filters.sort });
      if (my !== seq) return; // a newer request superseded this one
      library.papers = papers;
      searchMeta.byId = {};
    } else {
      const keywordOnly = Boolean(opts?.keywordOnly) || !engines.semantic;
      const resp = await searchPapers(filters.q, engines, keywordOnly);
      if (my !== seq) return;
      library.papers = resp.results.map((r) => r.paper);
      searchMeta.byId = Object.fromEntries(resp.results.map((r) => [r.paper.id, r.match]));
      searchMeta.semantic = { available: resp.semantic.available, reason: resp.semantic.reason };
    }
  } catch (e) {
    if (my === seq) library.error = errorMessage(e, 'loading the library failed');
  } finally {
    if (my === seq) {
      library.loading = false;
      library.loaded = true;
    }
  }
}

/// Download the rows the table shows, in its order, as one .bib file — the
/// same set whether it came from a filter or a ranked search.
export async function exportShownPapers(): Promise<void> {
  try {
    const blob = await exportPapers({ ids: library.papers.map((p) => p.id), format: bibFormat.value });
    downloadBlob(blob, 'xuewen.bib');
  } catch (e) {
    toast('error', `Couldn't export: ${errorMessage(e, 'export failed')}`);
  }
}

export async function loadDetail(id: string): Promise<PaperDetail> {
  const cached = cachedDetail(id);
  if (cached) return cached;
  const d = await getPaper(id);
  setDetail(id, d);
  return d;
}

/// Flip a paper's starred flag optimistically: patch the row/cached detail
/// first so the star moves instantly, then call the API and roll back (with
/// an error toast) if it rejects. When the starred filter is active the list
/// itself may need to drop/gain the paper, so it reloads after the call.
export async function toggleStar(paperId: string): Promise<void> {
  const row = library.papers.find((p) => p.id === paperId);
  const prev = row?.starred ?? cachedDetail(paperId)?.starred ?? false;
  const next = !prev;
  if (row) row.starred = next;
  patchDetail(paperId, { starred: next });
  try {
    await setStar(paperId, next);
  } catch (e) {
    if (row) row.starred = prev;
    patchDetail(paperId, { starred: prev });
    toast('error', `Couldn't update star: ${errorMessage(e, 'star failed')}`);
    return;
  }
  if (filters.starred) await loadPapers();
}
