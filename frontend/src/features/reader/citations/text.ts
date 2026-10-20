import {
  assignColumns,
  clusterLines,
  cmCompare,
  columnOfX,
  isReferencesHeading,
  LINE_TOLERANCE,
  type Marker,
  type PageText,
  type RefAnchor,
  type Reference,
  type TextRun,
} from './references';

export interface CmLine {
  pageIndex: number;
  col: number;
  y: number;
  x: number;
  text: string;
  runs: { run: TextRun; start: number; end: number }[];
}

/** Visual lines in column-major reading order, with char-offset→run mapping.
 *  Lines are reconstructed by the shared column-aware baseline clustering
 *  (see clusterLines) so drop-cap/small-caps headings reassemble correctly. */
export function columnMajorLines(page: PageText): CmLine[] {
  const cols = assignColumns(page.runs, page.width);
  const lines: CmLine[] = clusterLines(page.runs, cols).map((l) => {
    let text = '';
    const runs: CmLine['runs'] = [];
    for (const run of l.runs) {
      runs.push({ run, start: text.length, end: text.length + run.text.length });
      text += run.text;
    }
    return { pageIndex: page.pageIndex, col: l.col, y: l.y, x: l.x, text, runs };
  });
  return lines.sort(cmCompare);
}

export interface SegmentedRefs {
  references: Reference[];
  /** bib number n → reference index (numbered style only). */
  numberOf: Map<number, number>;
  style: 'numbered' | 'authoryear';
}

const MIN_ENTRIES = 2;
const NUMBERED_START = /^\s*\[(\d{1,3})\]\s*/;

/** Build Reference entries from entry-start line indexes: each entry spans
 *  its start line up to the next start, its raw text is the ws-collapsed
 *  join, and an in-PDF URL link near any of its lines (same page, same
 *  column, y within 2×LINE_TOLERANCE) becomes `externalUrl`. The column
 *  check matters on two-column pages: a DOI link in the other column can
 *  share a baseline with this entry's lines. Shared by the numbered and
 *  author-year segmentation paths, which differ only in how starts are
 *  found. */
function refsFromStarts(lines: CmLine[], pages: PageText[], starts: number[]): Reference[] {
  return starts.map((from, refIndex) => {
    const to = refIndex + 1 < starts.length ? starts[refIndex + 1] : lines.length;
    const span = lines.slice(from, to);
    const rawText = span.map((l) => l.text).join(' ').replace(/\s+/g, ' ').trim();
    const first = lines[from];
    const page = pages.find((p) => p.pageIndex === first.pageIndex);
    const externalUrl = page?.urlLinks.find((u) => {
      const col = columnOfX(page.runs, page.width, u.x);
      return span.some(
        (l) =>
          l.pageIndex === page.pageIndex &&
          l.col === col &&
          Math.abs(l.y - u.y) <= LINE_TOLERANCE * 2,
      );
    })?.url;
    return { index: refIndex, destPageIndex: first.pageIndex, destY: first.y, rawText, externalUrl };
  });
}

/** Segment the bibliography that starts at `refStart` into entries.
 *  Numbered path: entries begin at lines starting with "[n]". */
export function segmentReferences(pages: PageText[], refStart: RefAnchor): SegmentedRefs | null {
  // All lines at/after the heading, in reading order across pages.
  const ordered = [...pages].sort((a, b) => a.pageIndex - b.pageIndex);
  const startCol = colOfPoint(pages, refStart.pageIndex, refStart.x);
  const lines: CmLine[] = [];
  for (const p of ordered) {
    for (const l of columnMajorLines(p)) {
      const after =
        p.pageIndex > refStart.pageIndex ||
        (p.pageIndex === refStart.pageIndex &&
          (l.col > startCol || (l.col === startCol && l.y >= refStart.y - LINE_TOLERANCE)));
      if (after) lines.push(l);
    }
  }

  const starts = lines
    .map((l, i) => ({ i, m: l.text.match(NUMBERED_START) }))
    .filter((s): s is { i: number; m: RegExpMatchArray } => s.m !== null);
  if (starts.length >= MIN_ENTRIES) {
    const numberOf = new Map<number, number>();
    starts.forEach((s, refIndex) => numberOf.set(parseInt(s.m[1], 10), refIndex));
    const references = refsFromStarts(lines, pages, starts.map((s) => s.i));
    return { references, numberOf, style: 'numbered' };
  }

  // Author-year: entries split on the hanging-indent pattern, detected PER
  // COLUMN (entry starts sit at each column's own margin). For each column,
  // take its two most frequent line start-x buckets; whichever bucket's lines
  // contain a year more often marks that column's entry starts. A column that
  // is pure continuation spill-over can still contribute false starts if its
  // lines carry years — the global ≥60% year gate below bounds the damage.
  const YEAR = /(?:19|20)\d{2}/;
  const xKey = (x: number) => Math.round(x / 4) * 4;
  const startXByCol = new Map<number, number>();
  const cols = [...new Set(lines.map((l) => l.col))];
  for (const col of cols) {
    const colLines = lines.filter((l) => l.col === col);
    const freq = new Map<number, number>();
    for (const l of colLines) freq.set(xKey(l.x), (freq.get(xKey(l.x)) ?? 0) + 1);
    const candidates = [...freq.entries()].sort((a, b) => b[1] - a[1]).slice(0, 2).map(([x]) => x);
    if (candidates.length === 0) continue;
    const yearShare = (x: number) => {
      const at = colLines.filter((l) => xKey(l.x) === x);
      return at.length === 0 ? 0 : at.filter((l) => YEAR.test(l.text)).length / at.length;
    };
    const startX = candidates.length === 1 ? candidates[0]
      : yearShare(candidates[0]) >= yearShare(candidates[1]) ? candidates[0] : candidates[1];
    startXByCol.set(col, startX);
  }

  const ayStarts = lines
    .map((l, i) => ({ l, i }))
    .filter(({ l }) => startXByCol.get(l.col) === xKey(l.x) && !isReferencesHeading(l.text));
  if (ayStarts.length < MIN_ENTRIES) return null;

  const ayRefs = refsFromStarts(lines, pages, ayStarts.map(({ i }) => i));
  // Sanity: a real bibliography's entries overwhelmingly contain a year.
  const withYear = ayRefs.filter((r) => YEAR.test(r.rawText)).length;
  if (withYear / ayRefs.length < 0.6) return null;
  return { references: ayRefs, numberOf: new Map(), style: 'authoryear' };
}

function colOfPoint(pages: PageText[], pageIndex: number, x: number): number {
  const p = pages.find((pp) => pp.pageIndex === pageIndex);
  return p ? columnOfX(p.runs, p.width, x) : 0;
}

const BRACKET_GROUP = /\[(\d{1,3}(?:\s*[,;–—-]\s*\d{1,3})*)\]/g;
/** An unterminated group opening at end of line: "[14,19," or "[24–". */
const GROUP_TAIL = /\[(\d{1,3}(?:\s*[,;–—-]\s*\d{1,3})*\s*[,;–—-])\s*$/;
/** A group continuation at start of line: "44]" or "19, 44]". */
const GROUP_HEAD = /^\s*(\d{1,3}(?:\s*[,;–—-]\s*\d{1,3})*)\]/;

/** Bracketed-number citation markers in body lines. A group is a citation
 *  only if EVERY number in it (ranges expanded) resolves to a known entry —
 *  this kills math like [0, 1] and stray [17]s. Each comma-separated member
 *  of a group gets its OWN marker over its own characters, so hovering "19"
 *  in "[14,19,37]" shows entry 19, not entry 14 (a range keeps one marker at
 *  its first entry). A group split across a line break — "[14,19,\n44]" —
 *  is joined with the next line of the same page/column and validated whole,
 *  with each fragment's markers placed on its own line. */
export function findNumberedMarkers(bodyLines: CmLine[], numberOf: Map<number, number>): Marker[] {
  const markers: Marker[] = [];

  // One marker per digit-bearing part of `inner` (the text between the
  // brackets), each over its own characters. `whole` = the full bracketed
  // span; single-part groups keep it as their hitbox ("[3]" incl. brackets).
  const emitParts = (line: CmLine, inner: string, innerStart: number, whole: { start: number; end: number } | null) => {
    const parts: { text: string; start: number }[] = [];
    let off = 0;
    for (const piece of inner.split(/[,;]/)) {
      if (/\d/.test(piece)) parts.push({ text: piece, start: off });
      off += piece.length + 1;
    }
    for (const p of parts) {
      const n = parseInt(p.text.match(/\d{1,3}/)![0], 10);
      const refIndex = numberOf.get(n);
      if (refIndex === undefined) continue; // group pre-validated; belt & braces
      let s = innerStart + p.start;
      let e = s + p.text.length;
      if (whole && parts.length === 1) ({ start: s, end: e } = whole);
      const t = line.text.slice(s, e);
      const rect = rectForSpan(line, s + (t.length - t.trimStart().length), e - (t.length - t.trimEnd().length));
      if (rect) markers.push({ pageIndex: line.pageIndex, ...rect, refIndex });
    }
  };

  for (let i = 0; i < bodyLines.length; i++) {
    const line = bodyLines[i];
    for (const m of line.text.matchAll(BRACKET_GROUP)) {
      const nums = expandGroup(m[1]);
      if (!nums || nums.some((n) => !numberOf.has(n))) continue;
      emitParts(line, m[1], m.index! + 1, { start: m.index!, end: m.index! + m[0].length });
    }
    // Cross-line continuation, joined only within the same page and column.
    const next = bodyLines[i + 1];
    if (!next || next.pageIndex !== line.pageIndex || next.col !== line.col) continue;
    const tail = line.text.match(GROUP_TAIL);
    const head = tail ? next.text.match(GROUP_HEAD) : null;
    if (!tail || !head) continue;
    const nums = expandGroup(tail[1] + head[1]);
    if (!nums || nums.some((n) => !numberOf.has(n))) continue;
    emitParts(line, tail[1], line.text.length - tail[0].length + 1, null);
    emitParts(next, head[1], head[0].length - 1 - head[1].length, null);
  }
  return markers;
}

/** "3, 5" → [3,5]; "1–4" → [1,2,3,4]; null on a zero or an absurd range. */
function expandGroup(group: string): number[] | null {
  const out: number[] = [];
  for (const part of group.split(/[,;]/)) {
    const range = part.split(/[–—-]/).map((s) => parseInt(s.trim(), 10));
    if (range.some((n) => !Number.isFinite(n) || n <= 0)) return null;
    if (range.length === 1) out.push(range[0]);
    else if (range.length === 2 && range[1] > range[0] && range[1] - range[0] <= 50) {
      for (let n = range[0]; n <= range[1]; n++) out.push(n);
    } else return null;
  }
  return out.length > 0 ? out : null;
}

/** Rect covering line chars [start, end): union of the runs' slices, with a
 *  proportional cut inside partially-covered runs. */
function rectForSpan(
  line: CmLine,
  start: number,
  end: number,
): { x: number; y: number; width: number; height: number } | null {
  let x1 = Infinity, x2 = -Infinity, y1 = Infinity, y2 = -Infinity;
  for (const { run, start: rs, end: re } of line.runs) {
    const s = Math.max(start, rs);
    const e = Math.min(end, re);
    if (s >= e) continue;
    const chars = re - rs;
    const left = run.x + ((s - rs) / chars) * run.width;
    const right = run.x + ((e - rs) / chars) * run.width;
    x1 = Math.min(x1, left);
    x2 = Math.max(x2, right);
    y1 = Math.min(y1, run.y);
    y2 = Math.max(y2, run.y + run.height);
  }
  if (x1 === Infinity) return null;
  return { x: x1, y: y1, width: x2 - x1, height: y2 - y1 };
}

export interface AyCandidate {
  pageIndex: number; x: number; y: number; width: number; height: number;
  citeText: string;
}

const PAREN_CITE = /\(([^()]{0,200}?(?:19|20)\d{2}[a-z]?[^()]{0,40})\)/g;
const NARRATIVE_CITE = /\b([A-Z][\p{L}'’-]+(?:\s+(?:and|&)\s+[A-Z][\p{L}'’-]+|\s+et\s+al\.?)?)\s*\(\s*((?:19|20)\d{2})[a-z]?\s*\)/gu;

/** Candidate author-year citation spans in body lines (not yet resolved). */
export function findAuthorYearCandidates(bodyLines: CmLine[]): AyCandidate[] {
  const out: AyCandidate[] = [];
  for (const line of bodyLines) {
    const spans: { start: number; end: number; citeText: string }[] = [];
    for (const m of line.text.matchAll(NARRATIVE_CITE)) {
      spans.push({ start: m.index!, end: m.index! + m[0].length, citeText: `${m[1]}, ${m[2]}` });
    }
    for (const m of line.text.matchAll(PAREN_CITE)) {
      // skip if inside an already-captured narrative span
      if (spans.some((s) => m.index! >= s.start && m.index! < s.end)) continue;
      spans.push({ start: m.index!, end: m.index! + m[0].length, citeText: m[1] });
    }
    for (const s of spans) {
      const rect = rectForSpan(line, s.start, s.end);
      if (rect) out.push({ pageIndex: line.pageIndex, ...rect, citeText: s.citeText });
    }
  }
  return out;
}

/** First-author surname (lowercased) + year from a raw bibliography entry
 *  head — entries virtually always begin with the first author's name. */
export function entryHeadInfo(rawText: string): { surname: string | null; year: number | null } {
  const year = rawText.match(/(?:19|20)\d{2}/);
  const name = rawText.match(/^[^A-Za-z]*([A-Z][\p{L}'’-]+)/u);
  return { surname: name ? name[1].toLowerCase() : null, year: year ? parseInt(year[0], 10) : null };
}

/** Resolve candidates against the segmented entries: structured
 *  (first-author surname + year) when parsed, raw entry head otherwise.
 *  One marker per candidate whose first sub-cite resolves. */
export function resolveAuthorYearMarkers(cands: AyCandidate[], refs: Reference[]): Marker[] {
  const index = refs.map((r) => {
    if (r.structured?.authors?.length || r.structured?.year != null) {
      const first = r.structured.authors[0] ?? '';
      const parts = first.trim().split(/\s+/);
      return {
        refIndex: r.index,
        surname: (parts[parts.length - 1] ?? '').toLowerCase() || entryHeadInfo(r.rawText).surname,
        year: r.structured.year ?? entryHeadInfo(r.rawText).year,
      };
    }
    const head = entryHeadInfo(r.rawText);
    return { refIndex: r.index, surname: head.surname, year: head.year };
  });

  const markers: Marker[] = [];
  for (const c of cands) {
    let resolved: number | null = null;
    for (const sub of c.citeText.split(';')) {
      const year = sub.match(/(?:19|20)\d{2}/);
      if (!year) continue;
      const y = parseInt(year[0], 10);
      const words = (sub.match(/[A-Z][\p{L}'’-]+/gu) ?? []).map((w) => w.toLowerCase());
      const hit = index.find((e) => e.year === y && e.surname !== null && words.includes(e.surname));
      if (hit) { resolved = hit.refIndex; break; }
    }
    if (resolved !== null) {
      markers.push({ pageIndex: c.pageIndex, x: c.x, y: c.y, width: c.width, height: c.height, refIndex: resolved });
    }
  }
  return markers;
}
