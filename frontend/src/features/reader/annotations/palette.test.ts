import { describe, expect, it } from 'vitest';
import {
  ANNOTATION_COLORS,
  DEFAULT_ANNOTATION_COLOR,
  colorFromHex,
  colorHex,
  colorLabel,
  isAnnotationColor,
} from './palette';

describe('annotation palette', () => {
  it('offers exactly the five colors the backend enum accepts', () => {
    expect(ANNOTATION_COLORS).toEqual(['amber', 'rose', 'green', 'blue', 'violet']);
    expect(ANNOTATION_COLORS).toContain(DEFAULT_ANNOTATION_COLOR);
  });

  it('gives every color a distinct lowercase hex and a label', () => {
    const hexes = ANNOTATION_COLORS.map(colorHex);
    expect(new Set(hexes).size).toBe(hexes.length);
    for (const h of hexes) expect(h).toMatch(/^#[0-9a-f]{6}$/);
    for (const c of ANNOTATION_COLORS) expect(colorLabel(c)).not.toBe('');
  });

  it('round-trips a color through its hex, whatever the casing', () => {
    for (const c of ANNOTATION_COLORS) {
      expect(colorFromHex(colorHex(c))).toBe(c);
      expect(colorFromHex(colorHex(c).toUpperCase())).toBe(c);
      expect(colorFromHex(` ${colorHex(c)} `)).toBe(c);
    }
  });

  it('returns null for a hex outside the palette', () => {
    // A highlight baked into the PDF by another reader.
    expect(colorFromHex('#ffff00')).toBeNull();
    expect(colorFromHex('')).toBeNull();
  });

  it('guards unknown color names', () => {
    expect(isAnnotationColor('amber')).toBe(true);
    expect(isAnnotationColor('chartreuse')).toBe(false);
    expect(isAnnotationColor(null)).toBe(false);
    expect(isAnnotationColor(undefined)).toBe(false);
    // A prototype key must not read as a color.
    expect(isAnnotationColor('toString')).toBe(false);
  });
});
