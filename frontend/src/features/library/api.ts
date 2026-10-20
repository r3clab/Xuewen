import { request, requestText, requestVoid } from '$shared/api/http';
import { filterParams } from '$features/library/search/api';
import { hasSearchTerms, parseQuery } from '$features/library/search/query';
import type { BibFormat } from './types';
import type { Filters } from '$features/library/search/types';
import type { PaperDetail } from '$shared/api/generated/PaperDetail';
import type { PaperNameResponse } from '$shared/api/generated/PaperNameResponse';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';
import type { PreviewMeta } from '$shared/api/generated/PreviewMeta';
import type { Stats } from '$shared/api/generated/Stats';

export async function listPapers(f: Filters): Promise<PaperSummary[]> {
  return request(`/api/papers?${filterParams(f).toString()}`, 'list failed');
}

export async function getPaper(id: string): Promise<PaperDetail> {
  return request(`/api/papers/${encodeURIComponent(id)}`, 'detail failed');
}

export async function getStats(): Promise<Stats> {
  return request('/api/stats', 'stats failed');
}

/// How many pages the picker's preview should lay out, and page one's shape.
/// Rejects (422) when the PDF can't be rendered — the picker's cue to show a
/// text card instead of images.
export async function getPreviewMeta(id: string): Promise<PreviewMeta> {
  return request(`/api/papers/${encodeURIComponent(id)}/preview`, 'preview failed');
}

/// A rendered page, for an <img> src. Not a `request()` call: the browser
/// fetches, caches and lazy-loads these itself.
export function previewPageUrl(id: string, page: number): string {
  return `/papers/${encodeURIComponent(id)}/preview/${page}`;
}

export async function deletePaper(id: string): Promise<void> {
  return requestVoid(`/api/papers/${encodeURIComponent(id)}`, 'delete failed', {
    method: 'DELETE',
  });
}

/// Un-trash a soft-deleted paper (the delete toast's Undo).
export async function restorePaper(id: string): Promise<void> {
  return requestVoid(`/api/papers/${encodeURIComponent(id)}/restore`, 'restore failed', {
    method: 'POST',
  });
}

/// Returns the normalized name as the server stored it (trimmed; cleared to
/// null when empty) — callers treat that echo as authoritative, not their input.
export async function setPaperName(paperId: string, name: string | null): Promise<PaperNameResponse> {
  return request(`/api/papers/${encodeURIComponent(paperId)}/name`, 'update name failed', {
    method: 'PATCH',
    json: { name },
  });
}

export async function setStar(paperId: string, on: boolean): Promise<void> {
  return requestVoid(`/api/papers/${encodeURIComponent(paperId)}/star`, 'star failed', {
    method: on ? 'PUT' : 'DELETE',
  });
}

/// URL for one paper's citation export — also the Download link target
/// (sibling of `pdfUrl`; `exportUrl` below covers the filtered-list export).
export function paperExportUrl(id: string, fmt: BibFormat): string {
  return `/api/papers/${encodeURIComponent(id)}/export?format=${fmt}`;
}

export async function exportPaper(id: string, fmt: BibFormat): Promise<string> {
  return requestText(paperExportUrl(id, fmt), 'export failed');
}

/// URL for the filtered-list export. The export endpoint LIKE-matches `q`
/// against title/authors, so a qualifier-only query ('tag:nlp') must not leak
/// in as `q` — the parsed filters already carry it as tag=/project=/starred=/
/// status= (the same rule as loadPapers' listPapers call). Free-text/author
/// queries keep their `q`; the filter strip disables the export link for those.
export function exportUrl(f: Filters, fmt: BibFormat): string {
  const params = filterParams(hasSearchTerms(parseQuery(f.q)) ? f : { ...f, q: '' });
  params.set('format', fmt);
  return `/api/papers/export?${params.toString()}`;
}
