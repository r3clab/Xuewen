import { describe, expect, it, vi } from 'vitest';
import { PdfAnnotationSubtype } from '@embedpdf/models';
import type { PdfAnnotationObject } from '@embedpdf/models';
import {
  TOOL_BY_KIND,
  applyToolDefaults,
  canonicalJson,
  fromWire,
  kindOf,
  safeColor,
  sameAsStored,
  toWire,
} from './adapter';
import { colorHex } from './palette';
import type { Annotation } from '$shared/api/generated/Annotation';
import type { NewAnnotation } from '$shared/api/generated/NewAnnotation';

/** A highlight the way the plugin builds one from a text selection. */
function mark(over: Partial<PdfAnnotationObject> = {}): PdfAnnotationObject {
  return {
    id: 'a1',
    type: PdfAnnotationSubtype.HIGHLIGHT,
    pageIndex: 3,
    rect: { origin: { x: 10, y: 20 }, size: { width: 100, height: 12 } },
    color: colorHex('green'),
    strokeColor: colorHex('green'),
    opacity: 0.4,
    custom: { text: 'a quoted sentence' },
    ...over,
  } as PdfAnnotationObject;
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
    payload: { annotation: mark() },
    created_at: '2026-08-14T00:00:00Z',
    updated_at: '2026-08-14T00:00:00Z',
    ...over,
  };
}

describe('kindOf', () => {
  it('maps the five subtypes we store', () => {
    const cases: [PdfAnnotationSubtype, string][] = [
      [PdfAnnotationSubtype.HIGHLIGHT, 'highlight'],
      [PdfAnnotationSubtype.UNDERLINE, 'underline'],
      [PdfAnnotationSubtype.STRIKEOUT, 'strikeout'],
      [PdfAnnotationSubtype.SQUIGGLY, 'squiggly'],
      [PdfAnnotationSubtype.TEXT, 'text_comment'],
    ];
    for (const [type, kind] of cases) expect(kindOf({ type })).toBe(kind);
  });

  it('refuses every other subtype', () => {
    for (const type of [
      PdfAnnotationSubtype.INK,
      PdfAnnotationSubtype.FREETEXT,
      PdfAnnotationSubtype.LINK,
      PdfAnnotationSubtype.STAMP,
      PdfAnnotationSubtype.CARET,
      PdfAnnotationSubtype.UNKNOWN,
    ]) {
      expect(kindOf({ type })).toBeNull();
    }
  });
});

describe('toWire', () => {
  it('projects the queryable fields and keeps the payload verbatim', () => {
    const a = mark({ contents: 'worth revisiting' });
    const w = toWire({ annotation: a })!;
    expect(w).toMatchObject({
      page_index: 3,
      kind: 'highlight',
      color: 'green',
      quoted_text: 'a quoted sentence',
      note: 'worth revisiting',
    });
    expect(w.payload).toEqual({ annotation: a });
  });

  it('keeps a field it has never heard of, through the payload', () => {
    const a = mark({ someFutureField: 42 } as Partial<PdfAnnotationObject>);
    const w = toWire({ annotation: a })!;
    expect((w.payload as { annotation: Record<string, unknown> }).annotation.someFutureField).toBe(
      42,
    );
  });

  it('returns null for a subtype outside the whitelist', () => {
    expect(toWire({ annotation: mark({ type: PdfAnnotationSubtype.INK }) })).toBeNull();
  });

  it('treats blank quotes and notes as absent', () => {
    const w = toWire({ annotation: mark({ contents: '   ', custom: { text: '' } }) })!;
    expect(w.quoted_text).toBeNull();
    expect(w.note).toBeNull();
  });

  it('survives a custom field that is not the shape we expect', () => {
    for (const custom of [null, 'a string', 7, { text: 99 }]) {
      const w = toWire({ annotation: mark({ custom }) })!;
      expect(w.quoted_text).toBeNull();
    }
  });

  it('falls back to strokeColor, then to the default, for the palette name', () => {
    const stroke = toWire({
      annotation: mark({ color: undefined, strokeColor: colorHex('violet') }),
    })!;
    expect(stroke.color).toBe('violet');

    // A highlight baked into the PDF by another reader: keep the mark, lose
    // only the exact shade.
    const foreign = toWire({
      annotation: mark({ color: '#ffff00', strokeColor: '#ffff00' }),
    })!;
    expect(foreign.color).toBe('amber');
  });

  it('reads a sticky note from its stroke color', () => {
    const w = toWire({
      annotation: mark({
        type: PdfAnnotationSubtype.TEXT,
        color: undefined,
        strokeColor: colorHex('rose'),
        contents: 'a thought',
      }),
    })!;
    expect(w).toMatchObject({ kind: 'text_comment', color: 'rose', note: 'a thought' });
  });
});

describe('fromWire', () => {
  it('round-trips a mark back to the plugin', () => {
    const a = mark({ contents: 'note' });
    const back = fromWire(row({ payload: { annotation: a } }));
    expect(back).toEqual({ annotation: a });
  });

  it('is null when the backend degraded an unparseable payload', () => {
    expect(fromWire(row({ payload: null }))).toBeNull();
  });

  it('is null for a payload that is not an annotation envelope', () => {
    for (const payload of [{}, { annotation: null }, { annotation: 'x' }, 'x', 42]) {
      expect(fromWire(row({ payload }))).toBeNull();
    }
  });

  it('is null when the payload lost its id or type', () => {
    const { id: _id, ...noId } = mark();
    expect(fromWire(row({ payload: { annotation: noId } }))).toBeNull();
    const { type: _t, ...noType } = mark();
    expect(fromWire(row({ payload: { annotation: noType } }))).toBeNull();
  });

  it('refuses a payload whose subtype is outside the whitelist', () => {
    // Defense in depth: the row said "highlight", the payload says ink.
    const payload = { annotation: mark({ type: PdfAnnotationSubtype.INK }) };
    expect(fromWire(row({ payload }))).toBeNull();
  });
});

describe('canonicalJson', () => {
  it('ignores key order but not array order', () => {
    expect(canonicalJson({ b: 1, a: { d: 2, c: 3 } })).toBe(
      canonicalJson({ a: { c: 3, d: 2 }, b: 1 }),
    );
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
    const live = toWire({ annotation: mark() })!;
    const echoed: NewAnnotation = {
      ...live,
      payload: JSON.parse(canonicalJson(live.payload)) as unknown,
    };
    expect(Object.keys((echoed.payload as { annotation: object }).annotation)).not.toEqual(
      Object.keys((live.payload as { annotation: object }).annotation),
    );
    expect(sameAsStored(live, echoed)).toBe(true);
  });

  it('is true for an unchanged mark and false once anything moves', () => {
    const a = toWire({ annotation: mark() })!;
    expect(sameAsStored(a, toWire({ annotation: mark() })!)).toBe(true);
    expect(sameAsStored(a, toWire({ annotation: mark({ pageIndex: 4 }) })!)).toBe(false);
    expect(sameAsStored(a, toWire({ annotation: mark({ contents: 'new' }) })!)).toBe(false);
    expect(sameAsStored(a, toWire({ annotation: mark({ color: colorHex('blue') }) })!)).toBe(false);
    const moved = mark({ rect: { origin: { x: 0, y: 0 }, size: { width: 1, height: 1 } } });
    expect(sameAsStored(a, toWire({ annotation: moved })!)).toBe(false);
  });
});

describe('tool mapping', () => {
  it('names a plugin tool for every kind', () => {
    expect(TOOL_BY_KIND).toEqual({
      highlight: 'highlight',
      underline: 'underline',
      strikeout: 'strikeout',
      squiggly: 'squiggly',
      text_comment: 'textComment',
    });
  });

  it('pushes a color into every tool default, stroke-only for the sticky note', () => {
    const cap = { setToolDefaults: vi.fn() };
    applyToolDefaults(cap, '#f59e0b');
    expect(cap.setToolDefaults).toHaveBeenCalledTimes(5);
    expect(cap.setToolDefaults).toHaveBeenCalledWith('highlight', {
      color: '#f59e0b',
      strokeColor: '#f59e0b',
    });
    // A sticky note is an icon: stroke only, no fill to color.
    expect(cap.setToolDefaults).toHaveBeenCalledWith('textComment', { strokeColor: '#f59e0b' });
  });
});

describe('safeColor', () => {
  it('passes palette names through and defaults anything else', () => {
    expect(safeColor('blue')).toBe('blue');
    expect(safeColor('chartreuse')).toBe('amber');
    expect(safeColor(undefined)).toBe('amber');
  });
});
