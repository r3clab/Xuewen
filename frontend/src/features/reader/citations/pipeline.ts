/// The reader's citation pipeline: extract → match against the library →
/// structured parse → merge → re-match → author-year resolution, publishing a
/// progressive update after each phase that changes what the overlay can
/// show. Lives here rather than in PdfDocument so the sequencing, the
/// cancellation points between awaits, and the structured/fallback merge are
/// unit-testable.

import { matchCitations, parseCitations } from './api';
import type { CitationData, Reference } from './references';
import { loadCitations, type CitationSource } from './load';
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

/// Library papers keyed by the reference index that cites them.
async function matchLibrary(refs: Reference[]): Promise<Map<number, PaperSummary>> {
  const matched = await matchCitations(
    refs.map((r) => ({ text: r.rawText, title: r.structured?.title ?? undefined })),
  );
  const out = new Map<number, PaperSummary>();
  matched.forEach((paper, i) => {
    if (paper) out.set(refs[i].index, paper);
  });
  return out;
}

/// Never rejects: the reader still works without citation hovers, so any
/// failure only logs — putting the catch here means no caller can forget it.
export async function runCitationPipeline(
  source: CitationSource,
  documentId: string,
  { isCancelled, onUpdate }: CitationPipelineHooks,
): Promise<void> {
  try {
    const data = await loadCitations(source);
    if (isCancelled()) return;
    // Markers work before any matching or parsing — publish early so hovers
    // come alive while the slower phases run.
    onUpdate({ citations: data });
    if (data.references.length === 0) return;

    const matches = await matchLibrary(data.references);
    if (isCancelled()) return;
    onUpdate({ matches });

    let refs = data.references;
    try {
      refs = mergeStructured(refs, await parseCitations(documentId, refs.map((r) => r.rawText)));
    } catch (err) {
      console.warn('citation parsing failed; keeping the raw references', err);
    }
    if (isCancelled()) return;
    // Fallback author-year markers resolve best with structured entries, and
    // degrade to raw entry heads when the parse failed.
    const extra = data.pendingAuthorYear?.length
      ? resolveAuthorYearMarkers(data.pendingAuthorYear, refs)
      : [];
    const citations = { references: refs, markers: [...data.markers, ...extra] };
    // Parsed titles match more references than raw text alone.
    const final = refs === data.references ? matches : await matchLibrary(refs);
    if (isCancelled()) return;
    onUpdate({ citations, matches: final });
  } catch (err) {
    console.warn('citation extraction failed', err); // reader still works
  }
}
