import type { PdfLinkTarget } from '@embedpdf/svelte/link';
import type { PageRef } from '@embedpdf/svelte/runtime';
import {
  buildCitationData, columnOfX, findReferencesStart,
  type CitationData, type GotoLink, type PageText, type RefAnchor, type TextRun, type UrlLink,
} from './references';
import {
  columnMajorLines, findAuthorYearCandidates, findNumberedMarkers, segmentReferences,
  type AyCandidate, type CmLine,
} from './text';

/// One link annotation, where it is on its page. Every position here — the
/// rect, a destination's coordinates, the text runs — is in page space,
/// points from the page's top-left with y going down, so they line up with
/// one another and with the overlay as they are.
export interface CitationLink {
  pageIndex: number;
  rect: { x: number; y: number; width: number; height: number };
  target: PdfLinkTarget | null;
}

/// What the pipeline reads from an open document, narrowed so it can be fed
/// without a PDF engine.
export interface CitationSource {
  /// Every page's size, in order.
  pages: readonly { width: number; height: number }[];
  links(): readonly CitationLink[];
  readRuns(pageIndex: number): Promise<TextRun[]>;
  /// -1 for a page the document doesn't have.
  pageIndexOf(page: PageRef): number;
}

function destOf(
  target: PdfLinkTarget | null,
  pageIndexOf: (page: PageRef) => number,
): { pageIndex: number; x: number | null; y: number | null } | null {
  if (target?.kind !== 'goto') return null;
  const dest = target.destination;
  const pageIndex = pageIndexOf(dest.page);
  if (pageIndex < 0) return null;
  if (dest.kind === 'xyz') return { pageIndex, x: dest.x, y: dest.y };
  if (dest.kind === 'fitH' || dest.kind === 'fitBH') return { pageIndex, x: null, y: dest.y };
  // All other kinds: no usable vertical position here — anchor the page top.
  return { pageIndex, x: null, y: null };
}

function uriOf(target: PdfLinkTarget | null): string | undefined {
  return target?.kind === 'uri' ? target.uri : undefined;
}

export interface CitationLoad extends CitationData {
  /** Author-year marker candidates awaiting entry resolution (fallback mode;
   *  the pipeline resolves them once the structured parse arrives). */
  pendingAuthorYear?: AyCandidate[];
}

export async function loadCitations(source: CitationSource): Promise<CitationLoad> {
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
    const size = source.pages[idx];
    const read = size
      ? source.readRuns(idx).then(
          (runs): PageText => ({
            pageIndex: idx, width: size.width, height: size.height,
            runs, urlLinks: urlLinksByPage.get(idx) ?? [],
          }),
        )
      : Promise.resolve(null);
    pageCache.set(idx, read);
    return read;
  };
  for (const link of source.links()) {
    const url = uriOf(link.target);
    if (url) {
      const arr = urlLinksByPage.get(link.pageIndex) ?? [];
      arr.push({ ...link.rect, url });
      urlLinksByPage.set(link.pageIndex, arr);
      continue;
    }
    const dest = destOf(link.target, source.pageIndexOf);
    if (!dest) continue;
    links.push({ pageIndex: link.pageIndex, ...link.rect, destPageIndex: dest.pageIndex, destY: dest.y ?? 0, destX: dest.x ?? 0 });
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
  return loadCitationsFromText(source.pages.length, readPage);
}

// How many pages from the end to scan for the References heading.
const MAX_HEADING_SCAN = 15;

async function loadCitationsFromText(
  pageCount: number,
  readPage: (idx: number) => Promise<PageText | null>,
): Promise<CitationLoad> {
  // Heading: scan from the last page backward (bibliographies live at the back).
  let refStart: RefAnchor | null = null;
  const lowest = Math.max(0, pageCount - MAX_HEADING_SCAN);
  for (let i = pageCount - 1; i >= lowest; i--) {
    const p = await readPage(i);
    if (!p) continue;
    const found = findReferencesStart([p]);
    if (found) { refStart = found; break; }
  }
  if (!refStart) return { references: [], markers: [] };

  // Entries: heading page through the last page.
  const refPages: PageText[] = [];
  for (let i = refStart.pageIndex; i < pageCount; i++) {
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
