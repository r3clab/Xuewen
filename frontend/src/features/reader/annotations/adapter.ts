/// Translation between the engine's annotations and the wire shape the backend
/// stores (src/annotations/mod.rs).
///
/// The backend keeps a typed projection (kind/color/page/quote/note) beside the
/// verbatim payload; this module produces both from one engine annotation, and
/// rebuilds what to restore from the payload alone. Everything here is pure —
/// no plugin capability, no fetch — so it can be tested without a PDF engine.

import type { Annotation, AnnotationBundle, AnnotationDraft, AnnotationRef } from '@embedpdf/svelte/annotation';
import type { PageRef } from '@embedpdf/svelte/runtime';
import type { Quad } from '$features/reader/pdf/pageText';
import { DEFAULT_ANNOTATION_COLOR, colorFromHex, colorHex, isAnnotationColor } from './palette';
import type { Annotation as StoredAnnotation } from '$shared/api/generated/Annotation';
import type { AnnotationColor } from '$shared/api/generated/AnnotationColor';
import type { AnnotationKind } from '$shared/api/generated/AnnotationKind';
import type { NewAnnotation } from '$shared/api/generated/NewAnnotation';

/// The plugin tool id behind each kind. Only these five tools are surfaced.
export const TOOL_BY_KIND = {
  highlight: 'highlight',
  underline: 'underline',
  strikeout: 'strikeout',
  squiggly: 'squiggly',
  text_comment: 'note',
} as const satisfies Record<AnnotationKind, string>;

/// What each kind is called in the UI — one copy, so the toolbar's tooltip and
/// the sidebar's row can never disagree about what a mark is.
export const KIND_LABELS: Record<AnnotationKind, string> = {
  highlight: 'Highlight',
  underline: 'Underline',
  strikeout: 'Strikeout',
  squiggly: 'Squiggly',
  text_comment: 'Note',
};

/// The fields of an engine annotation this module reads. Every engine
/// annotation is one; the rest of its fields travel untouched in the payload.
export interface MarkData {
  readonly subtype: string;
  readonly ref: AnnotationRef;
  readonly nm?: string | null;
  readonly color?: string | null;
  readonly contents?: string | null;
  readonly quadPoints?: readonly Quad[];
}

/// The subtype whitelist. An annotation whose subtype is absent here is never
/// persisted: it is either a kind we don't offer, or — much more likely — a
/// mark that came baked into the PDF from some other reader.
const KIND_BY_SUBTYPE = new Map<string, AnnotationKind>([
  ['highlight', 'highlight'],
  ['underline', 'underline'],
  ['strikeout', 'strikeout'],
  ['squiggly', 'squiggly'],
  ['text', 'text_comment'],
]);

/// The kind this annotation would be stored as, or null if we don't store it.
export function kindOf(a: Pick<MarkData, 'subtype'>): AnnotationKind | null {
  return KIND_BY_SUBTYPE.get(a.subtype) ?? null;
}

/// The reader's own note. `contents` is the PDF-native field for it, which
/// means an exported copy carries the note as a normal annotation popup.
function note(a: MarkData): string | null {
  return typeof a.contents === 'string' && a.contents.trim() !== '' ? a.contents : null;
}

/// Which palette color the mark carries. A mark drawn with a hex we never
/// wrote falls back to the default rather than being dropped — losing the
/// exact shade of somebody else's highlight beats losing the highlight.
function colorOf(a: MarkData): AnnotationColor {
  return (typeof a.color === 'string' ? colorFromHex(a.color) : null) ?? DEFAULT_ANNOTATION_COLOR;
}

/// The quads a text mark covers, for reading the passage under it; none for a
/// sticky note.
export function quadsOf(a: MarkData): readonly Quad[] {
  return a.quadPoints ?? [];
}

/// One engine annotation as the backend's create-or-replace body, or null when
/// it is not one of ours to store. `quotedText` is the passage under it, read
/// from the page (pdf/pageText.ts): the engine keeps no copy of it.
export function toWire(annotation: MarkData, pageIndex: number, quotedText: string): NewAnnotation | null {
  const kind = kindOf(annotation);
  if (!kind) return null;
  return {
    page_index: pageIndex,
    kind,
    color: colorOf(annotation),
    quoted_text: quotedText.trim() === '' ? null : quotedText,
    note: note(annotation),
    // Verbatim — exactly what a read returns — so a field this app has never
    // heard of survives the round trip, and the row goes straight back into
    // an import bundle.
    payload: { data: annotation },
  };
}

/// What a stored row brings back into the document.
export type Restorable =
  | { readonly kind: 'data'; readonly data: Annotation }
  /// A row saved by the 2.x reader, whose payload is that plugin's own object.
  /// Redrawn from its geometry under its old id, then saved again as `data`.
  | { readonly kind: 'legacy'; readonly pageIndex: number; readonly draft: AnnotationDraft };

function isObject(v: unknown): v is Record<string, unknown> {
  return v !== null && typeof v === 'object' && !Array.isArray(v);
}

/// Only what `toWire` itself wrote: a read of one of our kinds, under a name.
/// The payload is this app's own echo, so these checks are what stand between
/// a corrupted row and the import, not a full validation.
function isStoredData(v: unknown): v is Annotation {
  if (!isObject(v) || typeof v.subtype !== 'string' || !KIND_BY_SUBTYPE.has(v.subtype)) return false;
  if (typeof v.nm !== 'string' || v.nm === '') return false;
  const ref = v.ref;
  return isObject(ref) && isObject(ref.page) && typeof ref.page.objectNumber === 'number';
}

/// `@embedpdf/models` 2.x `PdfAnnotationSubtype`, for the rows the 2.x reader
/// stored: TEXT = 1, HIGHLIGHT = 9, UNDERLINE = 10, SQUIGGLY = 11,
/// STRIKEOUT = 12.
const LEGACY_SUBTYPES = new Map<number, 'text' | 'highlight' | 'underline' | 'squiggly' | 'strikeout'>([
  [1, 'text'],
  [9, 'highlight'],
  [10, 'underline'],
  [11, 'squiggly'],
  [12, 'strikeout'],
]);

interface LegacyRect {
  origin: { x: number; y: number };
  size: { width: number; height: number };
}

function isLegacyRect(v: unknown): v is LegacyRect {
  if (!isObject(v) || !isObject(v.origin) || !isObject(v.size)) return false;
  return (
    typeof v.origin.x === 'number' &&
    typeof v.origin.y === 'number' &&
    typeof v.size.width === 'number' &&
    typeof v.size.height === 'number'
  );
}

/// 2.x kept rects in the same page space 3.x uses — points from the page's
/// top-left — so a rect becomes the quad on its corners.
function quadOf({ origin: { x, y }, size: { width, height } }: LegacyRect): Quad {
  return {
    upperLeft: { x, y },
    upperRight: { x: x + width, y },
    lowerLeft: { x, y: y + height },
    lowerRight: { x: x + width, y: y + height },
  };
}

/// The color and note come from the row's own projection, which is in the
/// palette's terms and was kept in step with the payload.
function legacyDraft(row: StoredAnnotation, a: Record<string, unknown>): AnnotationDraft | null {
  const subtype = typeof a.type === 'number' ? LEGACY_SUBTYPES.get(a.type) : undefined;
  if (!subtype || !isLegacyRect(a.rect)) return null;
  const color = colorHex(safeColor(row.color));
  if (subtype === 'text') {
    const { origin, size } = a.rect;
    return {
      subtype,
      rect: { x: origin.x, y: origin.y, width: size.width, height: size.height },
      color,
      icon: 'comment',
      nm: row.id,
      contents: row.note,
    };
  }
  const segments = Array.isArray(a.segmentRects) ? a.segmentRects.filter(isLegacyRect) : [];
  if (segments.length === 0) return null;
  const quadPoints = segments.map(quadOf);
  if (subtype === 'highlight') {
    return { subtype, quadPoints, color, blendMode: 'multiply', nm: row.id, contents: row.note };
  }
  return { subtype, quadPoints, color, nm: row.id, contents: row.note };
}

/// What to restore for a stored row, or null when the payload can't be
/// redrawn — the backend degrades an unparseable payload to `null` rather
/// than losing the row, and a row is still worth listing in the panel even if
/// the mark itself can't come back.
export function fromWire(row: StoredAnnotation): Restorable | null {
  const payload = row.payload;
  if (!isObject(payload)) return null;
  if (isStoredData(payload.data)) {
    // The row's id is the mark's name; a payload that disagrees would come
    // back as a different row on the next save.
    return payload.data.nm === row.id ? { kind: 'data', data: payload.data } : null;
  }
  if (isObject(payload.annotation)) {
    const draft = legacyDraft(row, payload.annotation);
    return draft ? { kind: 'legacy', pageIndex: row.page_index, draft } : null;
  }
  return null;
}

/// A page of the document the rows go back into.
export interface RestorePage {
  readonly index: number;
  readonly ref: PageRef;
  readonly size: { readonly width: number; readonly height: number };
}

export interface RestorePlan {
  /// The rows that come back, by id.
  readonly ids: readonly string[];
  /// The rows the current reader saved, as one bundle: imported as one
  /// change with `RESTORE_OPTIONS`, each mark under its own name, author and
  /// dates.
  readonly bundle: AnnotationBundle | null;
  /// The rows the 2.x reader saved, each to create on its page.
  readonly legacy: readonly { readonly page: PageRef; readonly draft: AnnotationDraft }[];
}

/// Pages map by position — the row's page index — so a mark lands on its page
/// even if the file's page objects were renumbered since it was saved.
export const RESTORE_OPTIONS = { history: false, attribution: 'restore', pages: 'by-position' } as const;

/// What to put back into a document for its stored rows. A row whose payload
/// could not be rebuilt, or whose page the document doesn't have, is left out:
/// the engine never hears of it. It still shows in the annotations panel,
/// which reads the store directly.
export function planRestore(rows: readonly StoredAnnotation[], pages: readonly RestorePage[]): RestorePlan {
  const ids: string[] = [];
  const bundlePages = new Map<number, AnnotationBundle['pages'][number]>();
  const items: AnnotationBundle['items'][number][] = [];
  const legacy: { page: PageRef; draft: AnnotationDraft }[] = [];
  for (const row of rows) {
    const restorable = fromWire(row);
    const page = restorable ? pages[restorable.kind === 'data' ? row.page_index : restorable.pageIndex] : undefined;
    if (!restorable || !page) continue;
    if (restorable.kind === 'legacy') {
      ids.push(row.id);
      legacy.push({ page: page.ref, draft: restorable.draft });
      continue;
    }
    const { data } = restorable;
    const at = bundlePages.get(data.ref.page.objectNumber);
    if (at && at.position !== page.index) continue;
    ids.push(row.id);
    bundlePages.set(data.ref.page.objectNumber, { page: data.ref.page, position: page.index, size: page.size });
    // A bundle's refs only name its items to one another. Each stored one was
    // assigned in whichever session last saved the mark, so two rows can carry
    // the same one.
    items.push({
      data: { ...data, ref: { kind: 'objectNumber', page: data.ref.page, objectNumber: items.length + 1 } },
      resources: {},
    });
  }
  const bundle: AnnotationBundle | null =
    items.length === 0
      ? null
      : {
          format: 'embedpdf/annotations',
          version: 1,
          pages: [...bundlePages.values()].sort((a, b) => a.position - b.position),
          items,
          resources: {},
        };
  return { ids, bundle, legacy };
}

/// A value as a string that does not depend on key order. Load-bearing for the
/// comparison below: a payload built here keeps the engine's own field order,
/// but the copy that comes back from the server has been through
/// `serde_json::Value`, whose map is a BTreeMap — so the round trip returns the
/// same object with its keys alphabetized. A plain `JSON.stringify` of the two
/// therefore differs on key order alone, and every unchanged mark would look
/// changed. Array order is content, so it is left as-is; `undefined` members
/// are dropped exactly as `JSON.stringify` drops them.
export function canonicalJson(v: unknown): string {
  if (v === null || typeof v !== 'object') return JSON.stringify(v) ?? 'null';
  if (Array.isArray(v)) return `[${v.map(canonicalJson).join(',')}]`;
  const fields = Object.entries(v as Record<string, unknown>)
    .filter(([, value]) => value !== undefined)
    .sort(([x], [y]) => (x < y ? -1 : x > y ? 1 : 0))
    .map(([k, value]) => `${JSON.stringify(k)}:${canonicalJson(value)}`);
  return `{${fields.join(',')}}`;
}

/// The payload without the mark's `ref`: the ref names the object the engine
/// wrote the mark as in this session, and every import renumbers it.
function contentOf(payload: unknown): unknown {
  if (!isObject(payload) || !isObject(payload.data)) return payload;
  const { ref: _ref, ...data } = payload.data;
  return { ...payload, data };
}

/// Whether a stored row still matches what the engine holds, so the save loop
/// can skip a PUT that would change nothing. Compares the projection and the
/// payload; the payload carries geometry, so a moved mark differs here.
export function sameAsStored(a: NewAnnotation, b: NewAnnotation): boolean {
  return (
    a.page_index === b.page_index &&
    a.kind === b.kind &&
    a.color === b.color &&
    a.quoted_text === b.quoted_text &&
    a.note === b.note &&
    canonicalJson(contentOf(a.payload)) === canonicalJson(contentOf(b.payload))
  );
}

/// Guard for a color arriving from the server, which is a closed enum there
/// but plain JSON here.
export function safeColor(v: unknown): AnnotationColor {
  return isAnnotationColor(v) ? v : DEFAULT_ANNOTATION_COLOR;
}
