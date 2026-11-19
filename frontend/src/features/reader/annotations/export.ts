/// Building an annotated copy of a paper's PDF.
///
/// The library file is never written to — `papers.content_hash` has to stay
/// true of the bytes on disk. An export opens a SECOND, throwaway document from
/// the same URL, restores the sidecar rows into that one exactly as a tab does
/// (planRestore), and downloads it: the engine writes the marks into the copy
/// it hands back, and nothing else ever sees that document. The throwaway is
/// closed however the attempt ends.

import type { AnnotationBundle, AnnotationDraft } from '@embedpdf/svelte/annotation';
import type { PageRef } from '@embedpdf/svelte/runtime';
import { RESTORE_OPTIONS, planRestore, type RestorePage } from './adapter';
import type { Annotation } from '$shared/api/generated/Annotation';

/// The throwaway's annotation capability, narrowed so the flow can be tested
/// without a PDF engine.
export interface ExportScope {
  import(bundle: AnnotationBundle, options: typeof RESTORE_OPTIONS): Promise<unknown>;
  create(page: PageRef, draft: AnnotationDraft): Promise<unknown>;
}

export interface ExportDeps {
  /// Resolves once the document is ready; rejects with why it couldn't open.
  open(documentId: string): Promise<unknown>;
  close(documentId: string): Promise<unknown>;
  pages(documentId: string): readonly RestorePage[];
  /// Null when the document has no annotation capability to restore into.
  scope(documentId: string): ExportScope | null;
  download(documentId: string): Promise<Uint8Array<ArrayBuffer>>;
}

/// The throwaway's document id. Prefixed so it can never collide with a paper's
/// own id — those are the document ids of the open tabs (see PdfDeck), and
/// reusing one would hand the export someone's open document.
export function exportDocumentId(paperId: string): string {
  return `export:${paperId}`;
}

export async function buildAnnotatedPdf(paperId: string, rows: Annotation[], deps: ExportDeps): Promise<Blob> {
  const documentId = exportDocumentId(paperId);
  try {
    await deps.open(documentId);
    const scope = deps.scope(documentId);
    if (!scope) throw new Error('the PDF could not take annotations, so no annotated copy was made');
    // A row whose payload could not be rebuilt is skipped rather than failing
    // the export: the rest of the marks are still worth having.
    const plan = planRestore(rows, deps.pages(documentId));
    if (plan.bundle) await scope.import(plan.bundle, RESTORE_OPTIONS);
    for (const { page, draft } of plan.legacy) await scope.create(page, draft);
    return new Blob([await deps.download(documentId)], { type: 'application/pdf' });
  } finally {
    await deps.close(documentId).catch(() => {});
  }
}

/// Plugin errors are Errors with a message; anything else thrown on the way
/// gets a generic one rather than "[object Object]".
export function exportErrorMessage(e: unknown): string {
  const message = e instanceof Error ? e.message : null;
  return message ? message : 'exporting the annotated PDF failed';
}
