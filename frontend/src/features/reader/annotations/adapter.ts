/// Translation between the plugin's `AnnotationTransferItem` and the wire
/// shape the backend stores (src/annotations/mod.rs).
///
/// The backend keeps a typed projection (kind/color/page/quote/note) beside the
/// verbatim payload; this module produces both from one plugin object, and
/// rebuilds the plugin object from the payload alone. Everything here is pure —
/// no plugin capability, no fetch — so it can be tested without a PDF engine.

import { PdfAnnotationSubtype } from '@embedpdf/models';
import type { PdfAnnotationObject } from '@embedpdf/models';
import type { AnnotationTransferItem } from '@embedpdf/plugin-annotation';
import {
  DEFAULT_ANNOTATION_COLOR,
  colorFromHex,
  isAnnotationColor,
} from './palette';
import type { Annotation } from '$shared/api/generated/Annotation';
import type { AnnotationColor } from '$shared/api/generated/AnnotationColor';
import type { AnnotationKind } from '$shared/api/generated/AnnotationKind';
import type { NewAnnotation } from '$shared/api/generated/NewAnnotation';

/// The plugin tool id behind each kind. Only these five tools are surfaced.
export const TOOL_BY_KIND: Record<AnnotationKind, string> = {
  highlight: 'highlight',
  underline: 'underline',
  strikeout: 'strikeout',
  squiggly: 'squiggly',
  text_comment: 'textComment',
};

/// What each kind is called in the UI — one copy, so the toolbar's tooltip and
/// the sidebar's row can never disagree about what a mark is.
export const KIND_LABELS: Record<AnnotationKind, string> = {
  highlight: 'Highlight',
  underline: 'Underline',
  strikeout: 'Strikeout',
  squiggly: 'Squiggly',
  text_comment: 'Note',
};

/// The color patch for a kind. A sticky note is an icon: it has a stroke color
/// but no fill to color, and getting that wrong paints a black square over the
/// page. One copy, because the plugin has to be told this three times — when
/// tools are seeded, when the palette changes, and when a selected mark is
/// recolored.
export function colorPatch(kind: AnnotationKind, hex: string): Record<string, string> {
  return kind === 'text_comment' ? { strokeColor: hex } : { color: hex, strokeColor: hex };
}

/// Push the palette color into every tool's defaults. Tool defaults are
/// registry-GLOBAL in the plugin — one set of tools shared by every open
/// document — so this belongs to a registry-wide caller (PdfDeck, of which
/// there is one live instance), never a per-tab component. Structural
/// capability slice for the same testability reason as SyncScope.
export function applyToolDefaults(
  cap: { setToolDefaults(toolId: string, patch: Record<string, string>): void },
  hex: string,
): void {
  for (const [kind, toolId] of Object.entries(TOOL_BY_KIND)) {
    cap.setToolDefaults(toolId, colorPatch(kind as AnnotationKind, hex));
  }
}

/// The subtype whitelist. An annotation whose subtype is absent here is never
/// persisted: it is either a type we don't offer, or — much more likely — a
/// mark that came baked into the PDF from some other reader. `replaceText`
/// also produces STRIKEOUT and `insertText` produces CARET, but neither tool is
/// registered, so nothing else can reach these subtypes through our UI.
const KIND_BY_SUBTYPE = new Map<PdfAnnotationSubtype, AnnotationKind>([
  [PdfAnnotationSubtype.HIGHLIGHT, 'highlight'],
  [PdfAnnotationSubtype.UNDERLINE, 'underline'],
  [PdfAnnotationSubtype.STRIKEOUT, 'strikeout'],
  [PdfAnnotationSubtype.SQUIGGLY, 'squiggly'],
  [PdfAnnotationSubtype.TEXT, 'text_comment'],
]);

/// The kind this annotation would be stored as, or null if we don't store it.
export function kindOf(a: Pick<PdfAnnotationObject, 'type'>): AnnotationKind | null {
  return KIND_BY_SUBTYPE.get(a.type) ?? null;
}

/// The text the mark was drawn over. Text-selection tools put it in
/// `custom.text` at create time (plugin-annotation 2.14.4), so no separate
/// selection plumbing is needed — but `custom` is `any`, so check the shape.
function quotedText(a: PdfAnnotationObject): string | null {
  const custom: unknown = a.custom;
  if (!custom || typeof custom !== 'object') return null;
  const text = (custom as { text?: unknown }).text;
  return typeof text === 'string' && text.trim() !== '' ? text : null;
}

/// The reader's own note. `contents` is the PDF-native field for it, which
/// means an exported copy carries the note as a normal annotation popup.
function note(a: PdfAnnotationObject): string | null {
  return typeof a.contents === 'string' && a.contents.trim() !== '' ? a.contents : null;
}

/// Which palette color the mark carries. Text markup colors the fill; a sticky
/// note only has a stroke. A mark drawn with a hex we never wrote falls back to
/// the default rather than being dropped — losing the exact shade of somebody
/// else's highlight beats losing the highlight.
function colorOf(a: PdfAnnotationObject): AnnotationColor {
  const withColors = a as { color?: unknown; strokeColor?: unknown };
  for (const hex of [withColors.color, withColors.strokeColor]) {
    if (typeof hex !== 'string') continue;
    const named = colorFromHex(hex);
    if (named) return named;
  }
  return DEFAULT_ANNOTATION_COLOR;
}

/// One plugin item as the backend's create-or-replace body, or null when the
/// annotation is not one of ours to store.
export function toWire(item: AnnotationTransferItem): NewAnnotation | null {
  const a = item.annotation;
  const kind = kindOf(a);
  if (!kind) return null;
  return {
    page_index: a.pageIndex,
    kind,
    color: colorOf(a),
    quoted_text: quotedText(a),
    note: note(a),
    // Verbatim, so a field this app has never heard of survives the round
    // trip. `ctx` is dropped: it only carries stamp bitmaps, and stamps are
    // not a kind we store.
    payload: { annotation: a },
  };
}

/// The plugin item to replay for a stored row, or null when the payload can't
/// be redrawn — the backend degrades an unparseable payload to `null` rather
/// than losing the row, and a row is still worth listing in the panel even if
/// the mark itself can't come back.
export function fromWire(row: Annotation): AnnotationTransferItem | null {
  const payload: unknown = row.payload;
  if (!payload || typeof payload !== 'object') return null;
  const annotation = (payload as { annotation?: unknown }).annotation;
  if (!annotation || typeof annotation !== 'object') return null;
  const a = annotation as PdfAnnotationObject;
  if (typeof a.id !== 'string' || typeof a.type !== 'number') return null;
  if (!kindOf(a)) return null;
  return { annotation: a };
}

/// A value as a string that does not depend on key order. Load-bearing for the
/// comparison below: a payload built here keeps the plugin's own field order,
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

/// Whether a stored row still matches what the plugin holds, so the save loop
/// can skip a PUT that would change nothing. Compares the projection and the
/// payload; the payload carries geometry, so a moved mark differs here.
export function sameAsStored(a: NewAnnotation, b: NewAnnotation): boolean {
  return (
    a.page_index === b.page_index &&
    a.kind === b.kind &&
    a.color === b.color &&
    a.quoted_text === b.quoted_text &&
    a.note === b.note &&
    canonicalJson(a.payload) === canonicalJson(b.payload)
  );
}

/// Guard for a color arriving from the server, which is a closed enum there
/// but plain JSON here.
export function safeColor(v: unknown): AnnotationColor {
  return isAnnotationColor(v) ? v : DEFAULT_ANNOTATION_COLOR;
}
