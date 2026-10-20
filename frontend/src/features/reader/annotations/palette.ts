/// The fixed reader palette. The backend stores the *semantic* color name
/// (`AnnotationColor` in src/annotations/mod.rs), never a hex string, so this
/// file is the only place a color becomes pixels — restyling here reflows every
/// existing mark with no migration.
///
/// The annotation layer mounts outside the `data-pdf-appearance` wrapper, so
/// these colors are never passed through the dim/invert filters: one hex per
/// color has to read on a white page and on an inverted near-black one. That
/// rules out pastels (invisible on white) and near-blacks (invisible on dark);
/// each hex below is a mid-tone chosen to sit between the two.

import type { AnnotationColor } from '$shared/api/generated/AnnotationColor';

interface Swatch {
  label: string;
  /// The hex handed to the annotation's `color`/`strokeColor`.
  hex: string;
}

/// A color added to the backend enum reaches the generated binding and fails
/// to compile here until `PALETTE` has a hex for it.
const PALETTE: Record<AnnotationColor, Swatch> = {
  amber: { label: 'Amber', hex: '#e8a33d' },
  rose: { label: 'Rose', hex: '#e0607e' },
  green: { label: 'Green', hex: '#4fa96b' },
  blue: { label: 'Blue', hex: '#4a90d9' },
  violet: { label: 'Violet', hex: '#9b72d0' },
};

/// Palette order — drives the swatch row and the wire enum's variant order.
export const ANNOTATION_COLORS = Object.keys(PALETTE) as AnnotationColor[];

export const DEFAULT_ANNOTATION_COLOR: AnnotationColor = 'amber';

export function colorHex(c: AnnotationColor): string {
  return PALETTE[c].hex;
}

export function colorLabel(c: AnnotationColor): string {
  return PALETTE[c].label;
}

export function isAnnotationColor(v: unknown): v is AnnotationColor {
  // `Object.hasOwn`, not `in`: `in` walks the prototype chain, so a stray
  // 'toString' off the wire would read as a palette color.
  return typeof v === 'string' && Object.hasOwn(PALETTE, v);
}

/// The inverse of `colorHex`, for reading a color back off an annotation the
/// plugin built from our own tool defaults. Case-insensitive because PDFium
/// round-trips hex in its own casing. Returns null for a hex we never wrote —
/// e.g. a highlight that came baked into the PDF from another reader.
export function colorFromHex(hex: string): AnnotationColor | null {
  const want = hex.trim().toLowerCase();
  return ANNOTATION_COLORS.find((c) => PALETTE[c].hex === want) ?? null;
}
