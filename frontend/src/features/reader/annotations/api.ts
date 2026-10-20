import { request, requestVoid } from '$shared/api/http';
import type { Annotation } from '$shared/api/generated/Annotation';
import type { NewAnnotation } from '$shared/api/generated/NewAnnotation';

function annotationUrl(paperId: string, annotationId?: string): string {
  const base = `/api/papers/${encodeURIComponent(paperId)}/annotations`;
  return annotationId === undefined ? base : `${base}/${encodeURIComponent(annotationId)}`;
}

export async function listAnnotations(paperId: string): Promise<Annotation[]> {
  return request(annotationUrl(paperId), 'loading annotations failed');
}

/** Create or replace one mark. Idempotent: the id addresses the row, so a
 *  retried save after a dropped response can't duplicate it. */
export async function putAnnotation(
  paperId: string,
  annotationId: string,
  body: NewAnnotation,
): Promise<Annotation> {
  return request(annotationUrl(paperId, annotationId), 'saving the annotation failed', {
    method: 'PUT',
    json: body,
  });
}

export async function deleteAnnotation(paperId: string, annotationId: string): Promise<void> {
  return requestVoid(annotationUrl(paperId, annotationId), 'deleting the annotation failed', {
    method: 'DELETE',
  });
}
