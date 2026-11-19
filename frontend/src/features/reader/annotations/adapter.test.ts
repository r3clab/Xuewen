import { describe, expect, it } from 'vitest';
import {
  RESTORE_OPTIONS,
  TOOL_BY_KIND,
  canonicalJson,
  fromWire,
  kindOf,
  planRestore,
  quadsOf,
  safeColor,
  sameAsStored,
  toWire,
  type MarkData,
  type RestorePage,
} from './adapter';
import { colorHex } from './palette';
import type { Annotation } from '$shared/api/generated/Annotation';
import type { NewAnnotation } from '$shared/api/generated/NewAnnotation';

const PAGE_REF = { kind: 'objectNumber', objectNumber: 7 } as const;
const QUAD = {
  upperLeft: { x: 10, y: 20 },
  upperRight: { x: 110, y: 20 },
  lowerLeft: { x: 10, y: 32 },
  lowerRight: { x: 110, y: 32 },
};

/** A highlight the way the engine reads one back. */
function mark(over: Partial<MarkData> & Record<string, unknown> = {}): MarkData {
  return {
    subtype: 'highlight',
    ref: { kind: 'objectNumber', page: PAGE_REF, objectNumber: 41 },
    nm: 'a1',
    color: colorHex('green'),
    contents: null,
    quadPoints: [QUAD],
    ...over,
  };
}

function row(over: Partial<Annotation> = {}): Annotation {
  return {
    paper_id: 'p1',
    id: 'a1',
    page_index: 3,
    kind: 'highlight',
    color: 'green',
    quoted_text: 'a quoted sentence',
    note: null,
    payload: { data: mark() },
    created_at: '2026-08-14T00:00:00Z',
    updated_at: '2026-08-14T00:00:00Z',
    ...over,
  };
}

/// A row as the 2.x reader stored it: that plugin's own object, its subtype
/// a number, its geometry an origin and a size.
function legacyRow(annotation: Record<string, unknown>, over: Partial<Annotation> = {}): Annotation {
  return row({ id: 'old', payload: { annotation }, ...over });
}

const LEGACY_RECT = { origin: { x: 10, y: 20 }, size: { width: 100, height: 12 } };

function pages(count: number): RestorePage[] {
  return Array.from({ length: count }, (_, index) => ({
    index,
    ref: { kind: 'objectNumber', objectNumber: 100 + index },
    size: { width: 612, height: 792 },
  }));
}

describe('kindOf', () => {
  it('maps the five subtypes we store', () => {
    const cases: [string, string][] = [
      ['highlight', 'highlight'],
      ['underline', 'underline'],
      ['strikeout', 'strikeout'],
      ['squiggly', 'squiggly'],
      ['text', 'text_comment'],
    ];
    for (const [subtype, kind] of cases) expect(kindOf({ subtype })).toBe(kind);
  });

  it('refuses every other subtype', () => {
    for (const subtype of ['ink', 'freeText', 'link', 'stamp', 'caret', 'popup', 'widget']) {
      expect(kindOf({ subtype })).toBeNull();
    }
  });
});

describe('quadsOf', () => {
  it('reads a text mark’s quads, and none for a sticky note', () => {
    expect(quadsOf(mark())).toEqual([QUAD]);
    expect(quadsOf(mark({ subtype: 'text', quadPoints: undefined }))).toEqual([]);
  });
});

describe('toWire', () => {
  it('projects the queryable fields and keeps the payload verbatim', () => {
    const a = mark({ contents: 'worth revisiting' });
    const w = toWire(a, 3, 'a quoted sentence')!;
    expect(w).toMatchObject({
      page_index: 3,
      kind: 'highlight',
      color: 'green',
      quoted_text: 'a quoted sentence',
      note: 'worth revisiting',
    });
    expect(w.payload).toEqual({ data: a });
  });

  it('keeps a field it has never heard of, through the payload', () => {
    const w = toWire(mark({ someFutureField: 42 }), 0, '')!;
    expect((w.payload as { data: Record<string, unknown> }).data.someFutureField).toBe(42);
  });

  it('returns null for a subtype outside the whitelist', () => {
    expect(toWire(mark({ subtype: 'ink' }), 0, '')).toBeNull();
  });

  it('treats blank quotes and notes as absent', () => {
    const w = toWire(mark({ contents: '   ' }), 0, '  ')!;
    expect(w.quoted_text).toBeNull();
    expect(w.note).toBeNull();
  });

  it('falls back to the default palette color for a shade it never wrote', () => {
    // A highlight baked into the PDF by another reader: keep the mark, lose
    // only the exact shade.
    expect(toWire(mark({ color: '#ffff00' }), 0, '')!.color).toBe('amber');
    expect(toWire(mark({ color: null }), 0, '')!.color).toBe('amber');
  });

  it('reads a sticky note’s color and note', () => {
    const w = toWire(mark({ subtype: 'text', color: colorHex('rose'), contents: 'a thought' }), 0, '')!;
    expect(w).toMatchObject({ kind: 'text_comment', color: 'rose', note: 'a thought' });
  });
});

describe('fromWire', () => {
  it('gives back a stored read as it was saved', () => {
    const data = mark({ contents: 'note' });
    expect(fromWire(row({ payload: { data } }))).toEqual({ kind: 'data', data });
  });

  it('is null when the backend degraded an unparseable payload', () => {
    expect(fromWire(row({ payload: null }))).toBeNull();
  });

  it('is null for a payload that is not an annotation envelope', () => {
    for (const payload of [{}, { data: null }, { data: 'x' }, { annotation: 'x' }, 'x', 42]) {
      expect(fromWire(row({ payload }))).toBeNull();
    }
  });

  it('is null when the payload lost its name, its ref or its subtype', () => {
    const { nm: _nm, ...noName } = mark();
    expect(fromWire(row({ payload: { data: noName } }))).toBeNull();
    const { ref: _ref, ...noRef } = mark();
    expect(fromWire(row({ payload: { data: noRef } }))).toBeNull();
    const { subtype: _s, ...noSubtype } = mark();
    expect(fromWire(row({ payload: { data: noSubtype } }))).toBeNull();
  });

  it('refuses a payload whose subtype is outside the whitelist', () => {
    // Defense in depth: the row said "highlight", the payload says ink.
    expect(fromWire(row({ payload: { data: mark({ subtype: 'ink' }) } }))).toBeNull();
  });

  it('refuses a payload named for some other row', () => {
    // It would come back as a different row on the next save.
    expect(fromWire(row({ id: 'b2' }))).toBeNull();
  });

  describe('a row the 2.x reader saved', () => {
    it('redraws a text mark from its segment rects, under its old id', () => {
      const back = fromWire(
        legacyRow({ id: 'old', type: 10, pageIndex: 3, rect: LEGACY_RECT, segmentRects: [LEGACY_RECT] }, { note: 'n' }),
      );
      expect(back).toEqual({
        kind: 'legacy',
        pageIndex: 3,
        draft: { subtype: 'underline', quadPoints: [QUAD], color: colorHex('green'), nm: 'old', contents: 'n' },
      });
    });

    it('draws a highlight multiplied, the way the highlight tool does', () => {
      const back = fromWire(legacyRow({ type: 9, rect: LEGACY_RECT, segmentRects: [LEGACY_RECT] }));
      expect(back?.kind === 'legacy' && back.draft).toMatchObject({ subtype: 'highlight', blendMode: 'multiply' });
    });

    it('redraws a sticky note at its rect', () => {
      const back = fromWire(legacyRow({ type: 1, rect: LEGACY_RECT }, { kind: 'text_comment', color: 'rose' }));
      expect(back?.kind === 'legacy' && back.draft).toEqual({
        subtype: 'text',
        rect: { x: 10, y: 20, width: 100, height: 12 },
        color: colorHex('rose'),
        icon: 'comment',
        nm: 'old',
        contents: null,
      });
    });

    it('is null without geometry to redraw, or for a subtype it never stored', () => {
      expect(fromWire(legacyRow({ type: 9, rect: LEGACY_RECT }))).toBeNull();
      expect(fromWire(legacyRow({ type: 9, segmentRects: [LEGACY_RECT] }))).toBeNull();
      expect(fromWire(legacyRow({ type: 15, rect: LEGACY_RECT, segmentRects: [LEGACY_RECT] }))).toBeNull();
    });
  });
});

describe('planRestore', () => {
  it('imports positions by page index, under names that stay unique in the bundle', () => {
    const rows = [
      row({ id: 'a1', page_index: 2 }),
      // The same stored ref as a1: both were saved in sessions that happened
      // to write them as the same object.
      row({ id: 'b2', page_index: 2, payload: { data: mark({ nm: 'b2' }) } }),
    ];
    const plan = planRestore(rows, pages(4));
    expect(plan.ids).toEqual(['a1', 'b2']);
    expect(plan.bundle?.pages).toEqual([{ page: PAGE_REF, position: 2, size: { width: 612, height: 792 } }]);
    const refs = plan.bundle?.items.map((item) => item.data.ref);
    expect(refs).toEqual([
      { kind: 'objectNumber', page: PAGE_REF, objectNumber: 1 },
      { kind: 'objectNumber', page: PAGE_REF, objectNumber: 2 },
    ]);
    expect(plan.bundle?.items.map((item) => item.data.nm)).toEqual(['a1', 'b2']);
    expect(plan.legacy).toEqual([]);
  });

  it('orders the bundle’s pages by position', () => {
    const late = { kind: 'objectNumber', objectNumber: 9 } as const;
    const rows = [
      row({ id: 'a1', page_index: 3, payload: { data: mark({ nm: 'a1', ref: { kind: 'objectNumber', page: late, objectNumber: 1 } }) } }),
      row({ id: 'b2', page_index: 1, payload: { data: mark({ nm: 'b2' }) } }),
    ];
    expect(planRestore(rows, pages(4)).bundle?.pages.map((p) => p.position)).toEqual([1, 3]);
  });

  it('leaves out a row whose page the document lacks, or whose payload cannot be redrawn', () => {
    const plan = planRestore([row({ page_index: 9 }), row({ id: 'x', payload: null })], pages(4));
    expect(plan).toEqual({ ids: [], bundle: null, legacy: [] });
  });

  it('creates the 2.x rows on their page instead of importing them', () => {
    const plan = planRestore(
      [legacyRow({ type: 12, rect: LEGACY_RECT, segmentRects: [LEGACY_RECT] }, { page_index: 1 })],
      pages(4),
    );
    expect(plan.ids).toEqual(['old']);
    expect(plan.bundle).toBeNull();
    expect(plan.legacy).toEqual([{ page: pages(4)[1].ref, draft: expect.objectContaining({ subtype: 'strikeout', nm: 'old' }) }]);
  });

  it('brings marks back under their own names and dates, with no undo step', () => {
    expect(RESTORE_OPTIONS).toEqual({ history: false, attribution: 'restore', pages: 'by-position' });
  });
});

describe('canonicalJson', () => {
  it('ignores key order but not array order', () => {
    expect(canonicalJson({ b: 1, a: { d: 2, c: 3 } })).toBe(canonicalJson({ a: { c: 3, d: 2 }, b: 1 }));
    expect(canonicalJson([1, 2])).not.toBe(canonicalJson([2, 1]));
  });

  it('drops undefined members, the way JSON.stringify does', () => {
    expect(canonicalJson({ a: 1, b: undefined })).toBe(canonicalJson({ a: 1 }));
    expect(canonicalJson([undefined])).toBe('[null]');
  });
});

describe('sameAsStored', () => {
  it('sees through the key reordering of a server round trip', () => {
    // serde_json's map is a BTreeMap, so the payload comes back alphabetized;
    // comparing raw JSON.stringify output would call every mark changed and
    // re-PUT it on every benign update event.
    const live = toWire(mark(), 3, 'q')!;
    const echoed: NewAnnotation = { ...live, payload: JSON.parse(canonicalJson(live.payload)) as unknown };
    expect(Object.keys((echoed.payload as { data: object }).data)).not.toEqual(
      Object.keys((live.payload as { data: object }).data),
    );
    expect(sameAsStored(live, echoed)).toBe(true);
  });

  it('ignores the ref, which every session renumbers', () => {
    const a = toWire(mark(), 3, 'q')!;
    const renumbered = toWire(mark({ ref: { kind: 'objectNumber', page: PAGE_REF, objectNumber: 99 } }), 3, 'q')!;
    expect(sameAsStored(a, renumbered)).toBe(true);
  });

  it('is true for an unchanged mark and false once anything moves', () => {
    const a = toWire(mark(), 3, 'q')!;
    expect(sameAsStored(a, toWire(mark(), 3, 'q')!)).toBe(true);
    expect(sameAsStored(a, toWire(mark(), 4, 'q')!)).toBe(false);
    expect(sameAsStored(a, toWire(mark({ contents: 'new' }), 3, 'q')!)).toBe(false);
    expect(sameAsStored(a, toWire(mark({ color: colorHex('blue') }), 3, 'q')!)).toBe(false);
    const moved = { ...QUAD, upperLeft: { x: 0, y: 0 } };
    expect(sameAsStored(a, toWire(mark({ quadPoints: [moved] }), 3, 'q')!)).toBe(false);
  });
});

describe('tool mapping', () => {
  it('names a plugin tool for every kind', () => {
    expect(TOOL_BY_KIND).toEqual({
      highlight: 'highlight',
      underline: 'underline',
      strikeout: 'strikeout',
      squiggly: 'squiggly',
      text_comment: 'note',
    });
  });
});

describe('safeColor', () => {
  it('passes palette names through and defaults anything else', () => {
    expect(safeColor('blue')).toBe('blue');
    expect(safeColor('chartreuse')).toBe('amber');
    expect(safeColor(undefined)).toBe('amber');
  });
});
