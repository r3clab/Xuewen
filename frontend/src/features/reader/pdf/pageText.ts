import { createCapabilityToken, definePlugin, type PageRef } from '@embedpdf/svelte/runtime';
import { isRotatedGeometryRun, type PageGeometryRun, type PageTextSnapshot } from '@embedpdf/engine';
import type { TextRun } from '$features/reader/citations/references';

interface Point {
  x: number;
  y: number;
}

/// Corner names as every engine quad has them: a text markup's `quadPoints`.
export interface Quad {
  upperLeft: Point;
  upperRight: Point;
  lowerLeft: Point;
  lowerRight: Point;
}

/// A page's text with where it is, for the reader's own consumers: the
/// citation pipeline's text runs and the passage a mark was drawn over. No
/// built-in plugin hands out either — selection reads only the user's range —
/// so this is a document plugin, the one place a document's engine handle is
/// reachable from the viewer.
export interface PageTextCapability {
  /// The page's text runs, in page space (points from the top-left).
  readRuns(pageIndex: number): Promise<TextRun[]>;
  /// The text a mark's quads cover, whitespace collapsed; '' when none.
  readTextUnder(page: PageRef, quads: readonly Quad[]): Promise<string>;
}

export const PageTextToken = createCapabilityToken<PageTextCapability>('page-text', {
  promises: { readRuns: true, readTextUnder: true },
});

interface PageText {
  snapshot: PageTextSnapshot;
  runs: readonly PageGeometryRun[];
}

/// The character→text direction of `charMap`. @embedpdf/engine exports only
/// the text→character one (`charRangeForTextOffsets`); this follows the
/// anchor rule documented on `PageTextSnapshot` (engine-core text/charmap.ts
/// `boundaryTextOffset`): the greatest anchor at or before the boundary, plus
/// the distance past it.
export function textOffsetOf(snapshot: PageTextSnapshot, boundary: number): number {
  const at = Math.min(Math.max(Math.floor(boundary), 0), snapshot.charCount);
  const map = snapshot.charMap;
  if (!map || map.length === 0) return Math.min(at, snapshot.text.length);
  let lo = 0;
  let hi = map.length - 1;
  let char = 0;
  let text = 0;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (map[mid][0] <= at) {
      [char, text] = map[mid];
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return text + (at - char);
}

function sliceText(snapshot: PageTextSnapshot, start: number, end: number): string {
  return snapshot.text.slice(textOffsetOf(snapshot, start), textOffsetOf(snapshot, end));
}

export function runsOf(page: PageText): TextRun[] {
  return page.runs.map((run) => ({
    text: sliceText(page.snapshot, run.start, run.start + run.glyphs.length),
    x: run.rect.x,
    y: run.rect.y,
    width: run.rect.width,
    height: run.rect.height,
  }));
}

function glyphCenter(run: PageGeometryRun, index: number): Point | null {
  if (isRotatedGeometryRun(run)) {
    const glyph = run.glyphs[index];
    if (glyph.empty) return null;
    const { upperLeft, upperRight, lowerLeft, lowerRight } = glyph.loose;
    return {
      x: (upperLeft.x + upperRight.x + lowerLeft.x + lowerRight.x) / 4,
      y: (upperLeft.y + upperRight.y + lowerLeft.y + lowerRight.y) / 4,
    };
  }
  const glyph = run.glyphs[index];
  if (glyph.empty) return null;
  return { x: glyph.loose.x + glyph.loose.width / 2, y: glyph.loose.y + glyph.loose.height / 2 };
}

function inside(point: Point, quad: Quad): boolean {
  const xs = [quad.upperLeft.x, quad.upperRight.x, quad.lowerLeft.x, quad.lowerRight.x];
  const ys = [quad.upperLeft.y, quad.upperRight.y, quad.lowerLeft.y, quad.lowerRight.y];
  return (
    point.x >= Math.min(...xs) &&
    point.x <= Math.max(...xs) &&
    point.y >= Math.min(...ys) &&
    point.y <= Math.max(...ys)
  );
}

/// A mark covers one contiguous stretch of characters — it was drawn from a
/// text selection — so the text from the first covered character to the last
/// is the passage, line breaks and dehyphenation as the engine reads them.
export function textUnder(page: PageText, quads: readonly Quad[]): string {
  let first = Infinity;
  let last = -Infinity;
  for (const run of page.runs) {
    for (let i = 0; i < run.glyphs.length; i++) {
      const center = glyphCenter(run, i);
      if (!center || !quads.some((quad) => inside(center, quad))) continue;
      first = Math.min(first, run.start + i);
      last = Math.max(last, run.start + i);
    }
  }
  if (first > last) return '';
  return sliceText(page.snapshot, first, last + 1).replace(/\s+/g, ' ').trim();
}

export const pageTextPlugin = definePlugin({
  id: 'page-text',
  token: PageTextToken,
  scope: 'document',
  create: (ctx) => {
    // Keyed by object number: a page's text changes only with its content,
    // which this reader never edits.
    const pages = new Map<number, Promise<PageText>>();
    const read = (ref: PageRef): Promise<PageText> => {
      const hit = pages.get(ref.objectNumber);
      if (hit) return hit;
      const handle = ctx.doc.page(ref);
      const loaded = Promise.all([handle.text.get(), handle.text.layout()]).then(([snapshot, layout]) => ({
        snapshot,
        runs: layout.runs,
      }));
      pages.set(ref.objectNumber, loaded);
      // A failed read is not cached: the next caller tries again.
      loaded.catch(() => pages.delete(ref.objectNumber));
      return loaded;
    };
    return {
      api: {
        async readRuns(pageIndex: number): Promise<TextRun[]> {
          const page = ctx.getPage(pageIndex);
          return page ? runsOf(await read(page.ref)) : [];
        },
        async readTextUnder(page: PageRef, quads: readonly Quad[]): Promise<string> {
          return textUnder(await read(page), quads);
        },
      },
    };
  },
});
