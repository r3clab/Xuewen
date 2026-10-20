/// The reader's citation pipeline: extract → library index → match →
/// structured parse → merge → author-year resolution, publishing a progressive
/// update after each phase that changes what the overlay can show. Lives here
/// rather than in PdfPages so the sequencing, the cancellation points between
/// awaits, and the structured/fallback merge are unit-testable; the component
/// keeps only the scheduling shell (the per-document one-shot guard,
/// `$state.snapshot`, `runWhenIdle`).

import type { PdfDocumentObject } from '@embedpdf/models';
import { parseCitations } from './api';
import { libraryTitleIndex, matchReferences } from './match';
import type { CitationData } from './references';
import { loadCitations, type EngineLike } from './load';
import { mergeStructured } from './refMerge';
import { resolveAuthorYearMarkers } from './text';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

export interface CitationPipelineUpdate {
  citations?: CitationData;
  matches?: Map<number, PaperSummary>;
}

export interface CitationPipelineHooks {
  /// Checked after every await; a cancelled run publishes nothing more.
  isCancelled: () => boolean;
  onUpdate: (update: CitationPipelineUpdate) => void;
}

/// Never rejects: the reader still works without citation hovers, so any
/// failure only logs — putting the catch here means no caller can forget it.
export async function runCitationPipeline(
  engine: EngineLike,
  doc: PdfDocumentObject,
  documentId: string,
  { isCancelled, onUpdate }: CitationPipelineHooks,
): Promise<void> {
  try {
    const data = await loadCitations(engine, doc);
    if (isCancelled()) return;
    // Markers work before any matching or parsing — publish early so hovers
    // come alive while the slower phases run.
    onUpdate({ citations: data });
    // Whole-library title index, independent of the current UI filter and
    // shared across all open tabs (one fetch + normalization pass).
    const index = await libraryTitleIndex();
    if (isCancelled()) return;
    onUpdate({ matches: matchReferences(data.references, index) });

    let refs = data.references;
    if (refs.length > 0) {
      // Structured upgrade — one POST per open; failure keeps raw text.
      const structured = await parseCitations(documentId, refs.map((r) => r.rawText));
      if (isCancelled()) return;
      if (structured) refs = mergeStructured(refs, structured);
    }
    // Fallback author-year markers resolve best with structured entries, and
    // degrade to raw entry heads when the parse is unavailable.
    const extra = data.pendingAuthorYear?.length
      ? resolveAuthorYearMarkers(data.pendingAuthorYear, refs)
      : [];
    onUpdate({
      citations: { references: refs, markers: [...data.markers, ...extra] },
      matches: matchReferences(refs, index),
    });
  } catch (err) {
    console.warn('citation extraction failed', err); // reader still works
  }
}
