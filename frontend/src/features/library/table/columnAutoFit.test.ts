import { describe, expect, it } from 'vitest';
import { autoFitWidth, canvasMeasurer, fitToAvailable, measureColumnFromDom } from './columnAutoFit';

// Deterministic stand-in for canvas measurement: 10px per character.
const measure = (text: string) => text.length * 10;

describe('autoFitWidth', () => {
  it('fits the widest cell plus padding', () => {
    const px = autoFitWidth(
      [
        { text: 'ab', font: 'f' },
        { text: 'abcd', font: 'f' },
      ],
      { min: 0, max: 1000, padding: 8 },
      measure,
    );
    expect(px).toBe(48);
  });

  it('ignores empty cells and clamps to [min, max]', () => {
    expect(autoFitWidth([{ text: '', font: 'f' }], { min: 60, max: 100, padding: 8 }, measure)).toBe(60);
    expect(autoFitWidth([{ text: 'x'.repeat(50), font: 'f' }], { min: 60, max: 100, padding: 8 }, measure)).toBe(100);
  });
});

describe('measureColumnFromDom', () => {
  it('collects only the matching data-col cells', () => {
    const root = document.createElement('div');
    root.innerHTML = `
      <span data-col="venue">NDSS</span>
      <span data-col="venue">USENIX Security</span>
      <span data-col="title">a much longer decoy title</span>
    `;
    document.body.appendChild(root);
    try {
      const px = measureColumnFromDom(root, 'venue', { min: 0, max: 1000, padding: 0 }, measure);
      expect(px).toBe('USENIX Security'.length * 10);
    } finally {
      root.remove();
    }
  });
});

describe('canvasMeasurer', () => {
  it('returns a finite positive width even without a canvas 2D context', () => {
    // jsdom has no canvas package, so this exercises the estimate fallback.
    const px = canvasMeasurer('hello', '16px sans-serif');
    expect(Number.isFinite(px)).toBe(true);
    expect(px).toBeGreaterThan(0);
  });
});

describe('fitToAvailable', () => {
  const wide = { min: 0, max: 10_000 };

  it('expands into surplus space proportionally', () => {
    expect(fitToAvailable({ a: 100, b: 50 }, { a: wide, b: wide }, 300)).toEqual({ a: 200, b: 100 });
  });

  it('caps expansion at a column max; the rest takes the freed surplus', () => {
    expect(fitToAvailable({ a: 100, b: 50 }, { a: { min: 0, max: 120 }, b: wide }, 300)).toEqual({
      a: 120,
      b: 180,
    });
  });

  it('shrinks proportionally when over budget', () => {
    expect(fitToAvailable({ a: 300, b: 100 }, { a: wide, b: wide }, 200)).toEqual({ a: 150, b: 50 });
  });

  it('never takes a column below its minimum; the others absorb the shrink', () => {
    const out = fitToAvailable({ a: 300, b: 100 }, { a: wide, b: { min: 90, max: 10_000 } }, 200);
    expect(out.b).toBe(90);
    expect(out.a).toBe(109); // floor(300 * 110/300) — floor keeps the sum under budget
  });

  it('returns the minimums when even they overflow', () => {
    expect(
      fitToAvailable(
        { a: 300, b: 100 },
        { a: { min: 150, max: 10_000 }, b: { min: 100, max: 10_000 } },
        200,
      ),
    ).toEqual({ a: 150, b: 100 });
  });
});
