/// The reader's annotations, cached per paper and mirrored to the backend.
///
/// This module owns the cache/server relationship: the sidebar panel reads from
/// it, and `sync.ts` (which watches the PDF plugin) writes through it.
/// Nothing else talks to `/api/papers/{id}/annotations` directly.

import { deleteAnnotation, listAnnotations, putAnnotation } from './api';
import type { Annotation } from '$shared/api/generated/Annotation';
import type { NewAnnotation } from '$shared/api/generated/NewAnnotation';

export const annotations = $state<{
  /// paper id → annotation id → row.
  byPaper: Record<string, Record<string, Annotation>>;
  /// Papers whose list has come back at least once. A paper that is absent
  /// here has an *unknown* set of marks, which is not the same as none — the
  /// sync loop must not treat it as "nothing to import".
  loaded: Record<string, boolean>;
  error: Record<string, string | null>;
}>({ byPaper: {}, loaded: {}, error: {} });

function bucket(paperId: string): Record<string, Annotation> {
  annotations.byPaper[paperId] ??= {};
  return annotations.byPaper[paperId];
}

/// Reading order — the same ordering the backend and the CLI use, so the panel
/// never disagrees with `xuewen annotation list`.
export function annotationList(paperId: string): Annotation[] {
  return Object.values(annotations.byPaper[paperId] ?? {}).sort(
    (a, b) =>
      a.page_index - b.page_index ||
      a.created_at.localeCompare(b.created_at) ||
      a.id.localeCompare(b.id),
  );
}

export function isLoaded(paperId: string): boolean {
  return annotations.loaded[paperId] === true;
}

/// Fetch a paper's marks. Errors are recorded, not thrown: a reader that can't
/// reach the annotation endpoint should still show the PDF.
export async function loadAnnotations(paperId: string): Promise<void> {
  try {
    const rows = await listAnnotations(paperId);
    annotations.byPaper[paperId] = Object.fromEntries(rows.map((r) => [r.id, r]));
    annotations.loaded[paperId] = true;
    annotations.error[paperId] = null;
  } catch (e) {
    annotations.error[paperId] = (e as Error).message;
  }
}

/// Create or replace one mark. The cache updates from the server's echo, so a
/// row's `created_at`/`updated_at` are never guessed locally.
export async function saveAnnotation(
  paperId: string,
  id: string,
  body: NewAnnotation,
): Promise<void> {
  const saved = await putAnnotation(paperId, id, body);
  bucket(paperId)[id] = saved;
}

export async function removeAnnotation(paperId: string, id: string): Promise<void> {
  await deleteAnnotation(paperId, id);
  delete bucket(paperId)[id];
}

/// Forget a closed tab's cache. The rows stay on the server; this only drops
/// what an unopened paper has no reason to keep in memory.
export function dropAnnotations(paperId: string): void {
  delete annotations.byPaper[paperId];
  delete annotations.loaded[paperId];
  delete annotations.error[paperId];
}
