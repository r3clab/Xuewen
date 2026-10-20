export interface TextRun { text: string; x: number; y: number; width: number; height: number; }
export interface UrlLink { x: number; y: number; width: number; height: number; url: string; }
export interface PageText {
  pageIndex: number;
  width: number;
  height: number;
  runs: TextRun[];
  urlLinks: UrlLink[];
}
/** An internal GoTo link annotation: a citation marker (source) with a destination. */
export interface GotoLink {
  pageIndex: number; // page the marker sits on
  x: number; y: number; width: number; height: number; // marker rect (top-left, PDF points)
  destPageIndex: number;
  destY: number; // destination y in the same top-left space (0 = page top when unknown)
  destX: number; // destination x in PDF points (0 when unknown) — column assignment
}
export interface RefAnchor { pageIndex: number; y: number; x: number; }

// A line is a references heading if, with every non-letter removed, it is one
// of these tokens — optionally preceded by a small roman-numeral section
// number ("VII. References"; arabic numbers are digits and vanish with the
// non-letters). The prefix must be a WELL-FORMED numeral in 1..39
// (x{0,3}(ix|iv|v?i{0,3})) so ordinary words spelled from roman letters
// ("Mild", "Civil", appendix label "D") cannot smuggle a match. Whole-line
// anchoring avoids "see the references section" false positives; letters-only
// comparison keeps tolerance for headings split across runs ("R"+"EFERENCES").
const HEADING_TOKENS = ['references', 'bibliography', 'workscited', 'referencesandnotes', 'referencescited'];
const HEADING_RE = new RegExp(`^(?:x{0,3}(?:ix|iv|v?i{0,3}))?(?:${HEADING_TOKENS.join('|')})$`);

// Baseline slack (PDF points) for treating two runs' bottoms as the same line.
export const LINE_TOLERANCE = 3;

export function isReferencesHeading(lineText: string): boolean {
  return HEADING_RE.test(lineText.replace(/[^a-zA-Z]/g, '').toLowerCase());
}

/** One reconstructed visual line: its runs (x-sorted, left→right reading order),
 *  the column they belong to, and the line's min top-y / min x. */
export interface ClusteredLine { col: number; y: number; x: number; runs: TextRun[]; }

/**
 * Group runs into visual lines, COLUMN-AWARE and by BASELINE, not by a fixed
 * y-bucket. Shared by pageLines (heading detection) and columnMajorLines
 * (fallback segmentation) so both reconstruct lines the same robust way.
 *
 * Two prior failure modes this replaces (measured live — see task-21 report):
 *  - Column-blind `Math.round(y/LINE_TOLERANCE)` bucketing joined a two-column
 *    heading with the OTHER column's same-baseline body text, so the joined
 *    "line" no longer matched the whole-line heading regex. Fixed by clustering
 *    per `cols` (runs in different columns never merge).
 *  - Small-caps / drop-cap headings emit the tall initial ("R") as its own run
 *    with a SMALLER top-y than the rest ("EFERENCES"); `round(y/3)` dropped them
 *    into adjacent buckets and the heading never reassembled. Their glyphs share
 *    a BASELINE (equal bottom = y+height), so we cluster by vertical overlap of
 *    the [y, y+height] extents instead of by top-y. Overlap (not a plain
 *    bottom-within-LINE_TOLERANCE test) also keeps the existing synthetic
 *    drop-cap fixture — same top, heights differing by 4 (> LINE_TOLERANCE) —
 *    passing, since its extents still overlap. LINE_TOLERANCE is retained as a
 *    baseline-equality shortcut for zero-overlap edge cases.
 *
 * Sort-then-sweep (no fixed buckets — adjacent-bucket splits were the bug): sort
 * by (column, top-y); a run joins the current line when it is the same column and
 * its vertical extent overlaps the line's accumulated extent by at least half the
 * shorter height (or their bottoms are within LINE_TOLERANCE). Distinct lines sit
 * a full line-height apart, so they never merge.
 */
export function clusterLines(runs: TextRun[], cols: Map<TextRun, number>): ClusteredLine[] {
  // Rotated/vertical page furniture — e.g. IEEE Xplore's sidebar stamp, a
  // 10×431pt run observed live (empc) — vertically overlaps EVERY body line
  // in its column, and the accumulated-extent sweep below would chain the
  // whole column into one franken-line, shuffling citation groups across
  // paragraphs. A run far taller than the page's typical line AND taller
  // than it is wide cannot be horizontal body text: drop it. (Median is per
  // call, so a page whose only run is tall keeps it; drop-caps are ~1.2×,
  // nowhere near the 5× bar.)
  const heights = runs.map((r) => r.height).sort((a, b) => a - b);
  const median = heights[Math.floor((heights.length - 1) / 2)] ?? 0;
  const usable = runs.filter((r) => !(r.height > 5 * median && r.height > r.width));
  const sorted = [...usable].sort(
    (a, b) => (cols.get(a) ?? 0) - (cols.get(b) ?? 0) || a.y - b.y,
  );
  const groups: { col: number; top: number; bottom: number; runs: TextRun[] }[] = [];
  let cur: { col: number; top: number; bottom: number; runs: TextRun[] } | null = null;
  for (const r of sorted) {
    const col = cols.get(r) ?? 0;
    const top = r.y;
    const bottom = r.y + r.height;
    if (cur && col === cur.col) {
      const overlap = Math.min(cur.bottom, bottom) - Math.max(cur.top, top);
      const shorter = Math.min(cur.bottom - cur.top, r.height);
      const sameLine = overlap >= shorter / 2 || Math.abs(bottom - cur.bottom) <= LINE_TOLERANCE;
      if (sameLine) {
        cur.runs.push(r);
        cur.top = Math.min(cur.top, top);
        cur.bottom = Math.max(cur.bottom, bottom);
        continue;
      }
    }
    cur = { col, top, bottom, runs: [r] };
    groups.push(cur);
  }
  return groups.map((g) => {
    g.runs.sort((a, b) => a.x - b.x);
    return {
      col: g.col,
      y: Math.min(...g.runs.map((r) => r.y)),
      x: Math.min(...g.runs.map((r) => r.x)),
      runs: g.runs,
    };
  });
}

/** Group a page's runs into visual lines: text concatenated in reading (x)
 *  order, tagged with the line's top y. Column-aware (see clusterLines). */
function pageLines(page: PageText): { y: number; x: number; text: string }[] {
  const cols = assignColumns(page.runs, page.width);
  return clusterLines(page.runs, cols).map((l) => ({
    y: l.y,
    x: l.x,
    text: l.runs.map((r) => r.text).join(''),
  }));
}

/**
 * The first line (in reading order: page ascending, then y ascending) that is
 * exactly a references heading. Returns its page + y, or null. Reconstructs
 * lines from runs so a styled heading split across runs is still detected.
 */
export function findReferencesStart(pages: PageText[]): RefAnchor | null {
  const ordered = [...pages].sort((a, b) => a.pageIndex - b.pageIndex);
  for (const p of ordered) {
    const lines = pageLines(p).sort((a, b) => a.y - b.y);
    for (const line of lines) {
      if (isReferencesHeading(line.text)) {
        return { pageIndex: p.pageIndex, y: line.y, x: line.x };
      }
    }
  }
  return null;
}

/** Assign each run to a column (0 = left/only, 1 = right). A page is
 *  two-column when both halves are populated and almost nothing straddles the
 *  midline (full-width headers/captions are tolerated up to 15%). */
export function assignColumns(runs: TextRun[], pageWidth: number): Map<TextRun, number> {
  const mid = pageWidth / 2;
  const straddling = runs.filter((r) => r.x < mid - 6 && r.x + r.width > mid + 6).length;
  const hasLeft = runs.some((r) => r.x + r.width <= mid);
  const hasRight = runs.some((r) => r.x >= mid);
  const twoCol = hasLeft && hasRight && straddling <= runs.length * 0.15;
  const out = new Map<TextRun, number>();
  for (const r of runs) out.set(r, twoCol && r.x + r.width / 2 >= mid ? 1 : 0);
  return out;
}

import type { StructuredReference } from '$shared/api/generated/StructuredReference';

export interface Reference {
  index: number;
  destPageIndex: number;
  destY: number;
  rawText: string;
  externalUrl?: string;
  /** LLM-parsed fields; undefined = not (yet) parsed, null = unparseable. */
  structured?: StructuredReference | null;
}
export interface Marker {
  pageIndex: number; x: number; y: number; width: number; height: number;
  refIndex: number;
}
export interface CitationData { references: Reference[]; markers: Marker[]; }

// Two destinations are the same reference if in the same page+column within
// this many PDF points vertically (one line of slack).
const DEST_EPSILON = 6;

export interface CmPos { col: number; y: number; x: number; }
/** Column-major reading order: left column top→bottom, then right. The one
 *  comparator for "which of two positions reads first" — shared with the
 *  text-layer fallback (`citations/text.ts`). */
export const cmCompare = (a: CmPos, b: CmPos) => a.col - b.col || a.y - b.y || a.x - b.x;

/** Which column x falls in on a page whose runs are `runs`: 0 unless the
 *  page is two-column and x is right of the midline. The one home for the
 *  "is this two-column, and which side is x on" question. */
export function columnOfX(runs: TextRun[], pageWidth: number, x: number): number {
  const cols = assignColumns(runs, pageWidth);
  const twoCol = [...cols.values()].some((c) => c === 1);
  return twoCol && x >= pageWidth / 2 ? 1 : 0;
}

export function buildCitationData(links: GotoLink[], pages: PageText[], refStart: RefAnchor): CitationData {
  // Per-page column assignment + column-major (reading-order) run list.
  const layout = new Map<number, { twoCol: boolean; mid: number; ordered: { run: TextRun; pos: CmPos }[] }>();
  for (const p of pages) {
    const cols = assignColumns(p.runs, p.width);
    const twoCol = [...cols.values()].some((c) => c === 1);
    const ordered = p.runs
      .map((run) => ({ run, pos: { col: cols.get(run) ?? 0, y: run.y, x: run.x } }))
      .sort((a, b) => cmCompare(a.pos, b.pos));
    layout.set(p.pageIndex, { twoCol, mid: p.width / 2, ordered });
  }
  const colOf = (pageIndex: number, x: number): number => {
    const l = layout.get(pageIndex);
    return l?.twoCol && x >= l.mid ? 1 : 0;
  };

  // A destination is a citation target if it reads AFTER the heading:
  // later page, later column, or same column at/after the heading y.
  const startPos: CmPos = { col: colOf(refStart.pageIndex, refStart.x), y: refStart.y - DEST_EPSILON, x: 0 };
  const isAfterStart = (pageIndex: number, pos: CmPos) =>
    pageIndex > refStart.pageIndex || (pageIndex === refStart.pageIndex && cmCompare(pos, startPos) >= 0);

  const posOfLink = (l: GotoLink): CmPos => ({ col: colOf(l.destPageIndex, l.destX), y: l.destY, x: 0 });
  const citeLinks = links.filter((l) => isAfterStart(l.destPageIndex, posOfLink(l)));

  // Distinct destinations = reference anchors, deduped by (page, column, y±ε),
  // in reading order.
  const anchors: { destPageIndex: number; pos: CmPos }[] = [];
  for (const l of citeLinks) {
    const pos = posOfLink(l);
    const hit = anchors.find(
      (a) => a.destPageIndex === l.destPageIndex && a.pos.col === pos.col && Math.abs(a.pos.y - pos.y) <= DEST_EPSILON,
    );
    if (!hit) anchors.push({ destPageIndex: l.destPageIndex, pos });
  }
  anchors.sort((a, b) => a.destPageIndex - b.destPageIndex || cmCompare(a.pos, b.pos));

  // Raw text: the column-major run band from this anchor to the next anchor
  // on the same page (flows across the column break), else to page end.
  const references: Reference[] = anchors.map((a, i) => {
    const next = anchors[i + 1];
    const l = layout.get(a.destPageIndex);
    const from: CmPos = { col: a.pos.col, y: a.pos.y - DEST_EPSILON, x: 0 };
    const to: CmPos | null =
      next && next.destPageIndex === a.destPageIndex
        ? { col: next.pos.col, y: next.pos.y - DEST_EPSILON, x: 0 }
        : null;
    const band = (l?.ordered ?? []).filter(
      ({ pos }) => cmCompare(pos, from) >= 0 && (to === null || cmCompare(pos, to) < 0),
    );
    const rawText = band.map(({ run }) => run.text).join(' ').replace(/\s+/g, ' ').trim();
    const p = pages.find((pp) => pp.pageIndex === a.destPageIndex);
    const externalUrl = (p?.urlLinks ?? []).find((u) => {
      const pos: CmPos = { col: colOf(a.destPageIndex, u.x), y: u.y, x: u.x };
      return cmCompare(pos, from) >= 0 && (to === null || cmCompare(pos, to) < 0);
    })?.url;
    return { index: i, destPageIndex: a.destPageIndex, destY: a.pos.y, rawText, externalUrl };
  });

  // Map each cite link (marker) to its reference.
  const markers: Marker[] = [];
  for (const l of citeLinks) {
    const pos = posOfLink(l);
    const ref = references.find(
      (r, i) =>
        anchors[i].destPageIndex === l.destPageIndex &&
        anchors[i].pos.col === pos.col &&
        Math.abs(anchors[i].pos.y - pos.y) <= DEST_EPSILON,
    );
    if (!ref) continue;
    markers.push({ pageIndex: l.pageIndex, x: l.x, y: l.y, width: l.width, height: l.height, refIndex: ref.index });
  }

  return { references, markers: coalesceMarkers(markers) };
}

// natbib/hyperref splits one author-year citation into TWO link annotations
// (the author part and the year part) that share a destination, so
// "(Kang et al., 2022)" would render as two seemingly distinct hover boxes.
// Neighboring cites in a group sit just as close (~3pt), so proximity alone
// cannot distinguish — merging keys on the SAME target reference.
const MERGE_GAP = 12; // max horizontal gap (PDF points) between fragments

/** Merge markers that target the same reference, share a visual line
 *  (vertical overlap), and sit within MERGE_GAP horizontally. */
export function coalesceMarkers(markers: Marker[]): Marker[] {
  const byGroup = new Map<string, Marker[]>();
  for (const m of markers) {
    const key = `${m.pageIndex}:${m.refIndex}`;
    const arr = byGroup.get(key) ?? [];
    arr.push(m);
    byGroup.set(key, arr);
  }
  const out: Marker[] = [];
  for (const group of byGroup.values()) {
    group.sort((a, b) => a.y - b.y || a.x - b.x);
    let cur: Marker | null = null;
    for (const m of group) {
      const sameLine =
        cur !== null && m.y < cur.y + cur.height && cur.y < m.y + m.height;
      if (cur && sameLine && m.x - (cur.x + cur.width) <= MERGE_GAP && m.x >= cur.x) {
        const right = Math.max(cur.x + cur.width, m.x + m.width);
        const bottom = Math.max(cur.y + cur.height, m.y + m.height);
        cur.y = Math.min(cur.y, m.y);
        cur.width = right - cur.x;
        cur.height = bottom - cur.y;
      } else {
        if (cur) out.push(cur);
        cur = { ...m };
      }
    }
    if (cur) out.push(cur);
  }
  return out;
}
