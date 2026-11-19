import { request, requestRaw, requestText, requestVoid } from '$shared/api/http';
import type { BibFormat } from '$shared/api/generated/BibFormat';
import type { ExportPapersBody } from '$shared/api/generated/ExportPapersBody';
import type { ListParams } from '$shared/api/generated/ListParams';
import type { PaperDetail } from '$shared/api/generated/PaperDetail';
import type { PaperNameResponse } from '$shared/api/generated/PaperNameResponse';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';
import type { PreviewMeta } from '$shared/api/generated/PreviewMeta';
import type { SetPaperNameBody } from '$shared/api/generated/SetPaperNameBody';
import type { Stats } from '$shared/api/generated/Stats';

/// The library list. `q` is the raw search-box string: the server parses its
/// qualifiers with the same grammar /api/search uses.
export async function listPapers({ q, sort }: ListParams = {}): Promise<PaperSummary[]> {
  const params = new URLSearchParams();
  if (q?.trim()) params.set('q', q.trim());
  if (sort) params.set('sort', sort);
  return request(`/api/papers?${params.toString()}`, 'list failed');
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
    json: { name } satisfies SetPaperNameBody,
  });
}

export async function setStar(paperId: string, on: boolean): Promise<void> {
  return requestVoid(`/api/papers/${encodeURIComponent(paperId)}/star`, 'star failed', {
    method: on ? 'PUT' : 'DELETE',
  });
}

/// URL for one paper's citation export — also the Download link target
/// (sibling of `pdfUrl`).
export function paperExportUrl(id: string, fmt: BibFormat): string {
  return `/api/papers/${encodeURIComponent(id)}/export?format=${fmt}`;
}

export async function exportPaper(id: string, fmt: BibFormat): Promise<string> {
  return requestText(paperExportUrl(id, fmt), 'export failed');
}

/// The given papers as one `.bib` file, entries in `ids` order.
export async function exportPapers(body: ExportPapersBody): Promise<Blob> {
  const res = await requestRaw('/api/papers/export', 'export failed', { method: 'POST', json: body });
  return res.blob();
}
