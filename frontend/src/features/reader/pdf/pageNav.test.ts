import { describe, expect, it } from 'vitest';
import { clampPage } from './pageNav';

describe('clampPage', () => {
  it('accepts in-range pages', () => {
    expect(clampPage('3', 12)).toBe(3);
    expect(clampPage(' 12 ', 12)).toBe(12);
  });

  it('clamps out-of-range pages', () => {
    expect(clampPage('0', 12)).toBe(1);
    expect(clampPage('-4', 12)).toBe(1);
    expect(clampPage('99', 12)).toBe(12);
  });

  it('rounds fractional input', () => {
    expect(clampPage('2.7', 12)).toBe(3);
  });

  it('rejects non-numeric or empty input', () => {
    expect(clampPage('abc', 12)).toBe(null);
    expect(clampPage('', 12)).toBe(null);
    expect(clampPage('  ', 12)).toBe(null);
  });

  it('rejects everything when there are no pages', () => {
    expect(clampPage('1', 0)).toBe(null);
  });
});
