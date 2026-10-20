import { PdfActionType, PdfZoomMode, type PdfDocumentObject, type PdfLinkTarget, type PdfPageObject } from '@embedpdf/models';
import {
  buildCitationData, columnOfX, findReferencesStart,
  type CitationData, type GotoLink, type PageText, type RefAnchor, type TextRun, type UrlLink,
} from './references';
import {
  columnMajorLines, findAuthorYearCandidates, findNumberedMarkers, segmentReferences,
  type AyCandidate, type CmLine,
} from './text';

// A structural subset of @embedpdf/models' PdfAnnotationObject: just the
// fields this module reads off a link annotation. The real PdfAnnotationObject
// is a union (link/text/popup/stamp/...) whose PdfAnnotationObjectBase
// requires bookkeeping fields (`id`, etc.) this module never touches, so
// using it verbatim as EngineLike's return type made the fake test engine
// (which supplies only type/rect/target) fail `npm run check` with
// "Property 'id' is missing". `target`'s shape, however, is the real
// PdfLinkTarget union (destination vs. action), so destOf/uriOf below are
// checked by the compiler against the real discriminated union.
export interface EngineAnnotation {
  type: number;
  rect: { origin: { x: number; y: number }; size: { width: number; height: number } };
  target?: PdfLinkTarget;
}

export interface EngineLike {
  getPageAnnotations(doc: PdfDocumentObject, page: PdfPageObject): { toPromise(): Promise<EngineAnnotation[]> };
  getPageTextRuns(
    doc: PdfDocumentObject,
    page: PdfPageObject,
  ): { toPromise(): Promise<{ runs: { text: string; rect: { origin: { x: number; y: number }; size: { width: number; height: number } } }[] }> };
}

const LINK = 2; // PdfAnnotationSubtype.LINK

// Coordinate spaces (verified live against real PDFs):
//  - Annotation rects and text-run rects come back in TOP-LEFT device space
//    (y grows downward) — the same space the rendered page + CSS overlay use —
//    so they pass through unchanged (a flip mirrored the hover boxes vertically).
//  - GoTo destination `y`, however, is left in PDF USER space (BOTTOM-LEFT origin,
//    y grows upward), so it must be flipped to top-left before it can be lined up
//    with the reference text runs — otherwise a marker maps to the mirror-image
//    reference (e.g. the popover showed the wrong entry).

function destOf(
  target: PdfLinkTarget | undefined,
): { pageIndex: number; x: number | null; y: number | null } | null {
  if (!target) return null;
  const dest =
    target.type === 'destination'
      ? target.destination
      : target.action.type === PdfActionType.Goto || target.action.type === PdfActionType.RemoteGoto
        ? target.action.destination
        : null;
  if (!dest) return null;
  const zoom = dest.zoom;
  if (zoom?.mode === PdfZoomMode.XYZ) {
    return { pageIndex: dest.pageIndex, x: zoom.params.x, y: zoom.params.y };
  }
  // /FitH-family carries its `top` only in the raw view array.
  if (
    (zoom?.mode === PdfZoomMode.FitHorizontal || zoom?.mode === PdfZoomMode.FitBoundingBoxHorizontal) &&
    typeof dest.view?.[0] === 'number'
  ) {
    return { pageIndex: dest.pageIndex, x: null, y: dest.view[0] };
  }
  // All other / unhandled zoom modes: no usable vertical position here —
  // anchor the page TOP. (y:null must never go through the bottom-left→
  // top-left flip, which would turn "unknown" into "page bottom" and
  // mis-map every marker on that page.)
  return { pageIndex: dest.pageIndex, x: null, y: null };
}

function uriOf(target: PdfLinkTarget | undefined): string | undefined {
  return target?.type === 'action' && target.action.type === PdfActionType.URI ? target.action.uri : undefined;
}

export interface CitationLoad extends CitationData {
  /** Author-year marker candidates awaiting entry resolution (fallback mode;
   *  PdfPages resolves them once the structured parse arrives). */
  pendingAuthorYear?: AyCandidate[];
}

export async function loadCitations(engine: EngineLike, doc: PdfDocumentObject): Promise<CitationLoad> {
  // Pass 1 — link annotations from every page (cheap). These give the citation
  // markers + their destinations, plus any URL links (kept per page so a
  // reference's DOI/URL can be attached below).
  const links: GotoLink[] = [];
  const urlLinksByPage = new Map<number, UrlLink[]>();

  // The one place a page's text becomes a PageText. Caches the PROMISE, not
  // the result, so concurrent readers of the same page (pass 2's Promise.all)
  // still cost one worker round-trip — and shared with the text-layer
  // fallback below, so pages the hyperlink path already read are hits there
  // rather than second fetches. Reads urlLinksByPage lazily: every call
  // happens after pass 1 has filled it.
  const pageCache = new Map<number, Promise<PageText | null>>();
  const readPage = (idx: number): Promise<PageText | null> => {
    const hit = pageCache.get(idx);
    if (hit) return hit;
    const page = doc.pages[idx];
    const read = page
      ? engine
          .getPageTextRuns(doc, page)
          .toPromise()
          .then((textRuns): PageText => {
            const runs: TextRun[] = textRuns.runs.map((r) => ({
              text: r.text,
              x: r.rect.origin.x,
              y: r.rect.origin.y,
              width: r.rect.size.width,
              height: r.rect.size.height,
            }));
            return {
              pageIndex: idx, width: page.size.width, height: page.size.height,
              runs, urlLinks: urlLinksByPage.get(idx) ?? [],
            };
          })
      : Promise.resolve(null);
    pageCache.set(idx, read);
    return read;
  };
  // Pages are independent — fetch all annotation lists concurrently instead
  // of paying one serialized worker round-trip per page.
  const annosByPage = await Promise.all(
    doc.pages.map((page) => engine.getPageAnnotations(doc, page).toPromise()),
  );
  for (const [pageIdx, annos] of annosByPage.entries()) {
    const page = doc.pages[pageIdx];
    for (const a of annos) {
      if (a.type !== LINK) continue;
      const rect = { x: a.rect.origin.x, y: a.rect.origin.y, width: a.rect.size.width, height: a.rect.size.height };
      const url = uriOf(a.target);
      if (url) {
        const arr = urlLinksByPage.get(page.index) ?? [];
        arr.push({ ...rect, url });
        urlLinksByPage.set(page.index, arr);
        continue;
      }
      const dest = destOf(a.target);
      if (!dest) continue;
      // Flip a KNOWN y from PDF bottom-left space into top-left space; an
      // unknown y anchors the page top (see destOf).
      const destPage = doc.pages[dest.pageIndex];
      const destY = dest.y == null ? 0 : destPage ? destPage.size.height - dest.y : dest.y;
      links.push({ pageIndex: page.index, ...rect, destPageIndex: dest.pageIndex, destY, destX: dest.x ?? 0 });
    }
  }
  if (links.length > 0) {
    // Pass 2 — the bibliography lives on the destination pages; ALSO read the
    // page just before the earliest destination, because the "References"
    // heading often sits at the bottom of the previous page.
    const destPages = [...new Set(links.map((l) => l.destPageIndex))].sort((a, b) => a - b);
    const scanPages = destPages[0] > 0 ? [destPages[0] - 1, ...destPages] : destPages;
    // Same concurrency as pass 1: the per-page text-run reads are independent.
    const pages: PageText[] = (await Promise.all(scanPages.map(readPage))).filter(
      (p): p is PageText => p !== null,
    );

    const refStart = findReferencesStart(pages);
    if (refStart) {
      const data = buildCitationData(links, pages, refStart);
      if (data.markers.length > 0) return data;
    }
  }
  // Hyperlink path yielded nothing usable — text-layer fallback.
  return loadCitationsFromText(doc, readPage);
}

// How many pages from the end to scan for the References heading.
const MAX_HEADING_SCAN = 15;

async function loadCitationsFromText(
  doc: PdfDocumentObject,
  readPage: (idx: number) => Promise<PageText | null>,
): Promise<CitationLoad> {
  // Heading: scan from the last page backward (bibliographies live at the back).
  let refStart: RefAnchor | null = null;
  const lowest = Math.max(0, doc.pages.length - MAX_HEADING_SCAN);
  for (let i = doc.pages.length - 1; i >= lowest; i--) {
    const p = await readPage(i);
    if (!p) continue;
    const found = findReferencesStart([p]);
    if (found) { refStart = found; break; }
  }
  if (!refStart) return { references: [], markers: [] };

  // Entries: heading page through the last page.
  const refPages: PageText[] = [];
  for (let i = refStart.pageIndex; i < doc.pages.length; i++) {
    const p = await readPage(i);
    if (p) refPages.push(p);
  }
  const seg = segmentReferences(refPages, refStart);
  if (!seg) return { references: [], markers: [] };

  // Body lines: pages before the heading, plus the heading page's lines that
  // read BEFORE the heading (column-major).
  const bodyLines: CmLine[] = [];
  for (let i = 0; i <= refStart.pageIndex; i++) {
    const p = await readPage(i);
    if (!p) continue;
    for (const l of columnMajorLines(p)) {
      const beforeHeading =
        i < refStart.pageIndex ||
        l.col < colOfAnchor(p, refStart) ||
        (l.col === colOfAnchor(p, refStart) && l.y < refStart.y);
      if (beforeHeading) bodyLines.push(l);
    }
  }

  const markers = seg.numberOf.size > 0 ? findNumberedMarkers(bodyLines, seg.numberOf) : [];
  const pendingAuthorYear = seg.style === 'authoryear' ? findAuthorYearCandidates(bodyLines) : undefined;
  return { references: seg.references, markers, pendingAuthorYear };
}

function colOfAnchor(p: PageText, a: RefAnchor): number {
  return columnOfX(p.runs, p.width, a.x);
}
