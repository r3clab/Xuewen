/// Building an annotated copy of a paper's PDF.
///
/// The library file is never written to. `autoCommit: false` is what keeps
/// marks out of the open document — `papers.content_hash` has to stay true of
/// the bytes on disk — so an export opens a SECOND, throwaway PDFium document
/// from the same URL, replays the sidecar rows into that one, commits them
/// there (a real PDF annotation write, into a document nothing else can see),
/// and saves the result. The throwaway is closed however the attempt ends.

import type { AnnotationEvent, AnnotationTransferItem } from '@embedpdf/plugin-annotation';
import type { PdfDocumentObject } from '@embedpdf/models';
import { fromWire } from './adapter';
import type { Annotation } from '$shared/api/generated/Annotation';

/// The slice of the annotation plugin's per-document scope this needs, as
/// promises rather than the plugin's tasks — narrow on purpose, so the flow can
/// be tested without a PDF engine.
export interface ExportScope {
  importAnnotations(items: AnnotationTransferItem[]): void;
  commit(): Promise<unknown>;
  onAnnotationEvent(handler: (e: AnnotationEvent) => void): () => void;
}

export interface ExportDeps {
  open(documentId: string): Promise<PdfDocumentObject>;
  close(documentId: string): void;
  scope(documentId: string): ExportScope;
  save(doc: PdfDocumentObject): Promise<ArrayBuffer>;
  /// How long to wait for the throwaway's own annotations to load. The plugin
  /// announces that with a `loaded` event and stays silent if the read fails,
  /// so without a bound a broken document would leave the reader watching a
  /// spinner that never stops.
  timeoutMs?: number;
}

const DEFAULT_TIMEOUT_MS = 30_000;

/// The throwaway's document id. Prefixed so it can never collide with a paper's
/// own id — those are the document ids of the open tabs (see PdfDeck), and
/// reusing one would hand the export someone's open document.
export function exportDocumentId(paperId: string): string {
  return `export:${paperId}`;
}

export async function buildAnnotatedPdf(
  paperId: string,
  rows: Annotation[],
  deps: ExportDeps,
): Promise<Blob> {
  // A row whose payload could not be rebuilt is skipped rather than failing the
  // export: the rest of the marks are still worth having.
  const items = rows.map(fromWire).filter((item): item is AnnotationTransferItem => item !== null);

  const documentId = exportDocumentId(paperId);
  const scope = deps.scope(documentId);

  let signalLoaded = (): void => {};
  const loaded = new Promise<void>((resolve) => {
    signalLoaded = resolve;
  });
  // Subscribed before the document is opened: the plugin reads a PDF's own
  // annotations as soon as it loads, which can happen before `open` resolves.
  const off = scope.onAnnotationEvent((e) => {
    if (e.type === 'loaded') signalLoaded();
  });
  let timer: ReturnType<typeof setTimeout> | undefined;

  try {
    const doc = await deps.open(documentId);
    // Handed over before the wait, not after: an import that arrives early is
    // queued by the plugin and drained just before `loaded` fires, so this
    // works whichever side wins.
    scope.importAnnotations(items);
    await Promise.race([
      loaded,
      new Promise<never>((_, reject) => {
        timer = setTimeout(
          () => reject(new Error('the PDF did not finish loading, so no annotated copy was made')),
          deps.timeoutMs ?? DEFAULT_TIMEOUT_MS,
        );
      }),
    ]);
    await scope.commit();
    return new Blob([await deps.save(doc)], { type: 'application/pdf' });
  } finally {
    clearTimeout(timer);
    off();
    deps.close(documentId);
  }
}

/// Engine and plugin failures arrive as `PdfErrorReason` — a plain `{ code,
/// message }`, not an Error — so a message has to be dug out rather than read
/// off `.message` and hoped for.
export function exportErrorMessage(e: unknown): string {
  const message = (e as { message?: unknown } | null)?.message;
  return typeof message === 'string' && message !== ''
    ? message
    : 'exporting the annotated PDF failed';
}
