import { beforeEach, describe, expect, it } from 'vitest';
import { cyclePdfAppearance, initPdfAppearance, pdfAppearance } from './theme.svelte';

beforeEach(() => {
  localStorage.clear();
  pdfAppearance.mode = 'normal';
});

describe('pdf page appearance', () => {
  it('cycles normal → dim → invert → normal and persists each step', () => {
    cyclePdfAppearance();
    expect(pdfAppearance.mode).toBe('dim');
    expect(localStorage.getItem('xuewen-pdf-appearance')).toBe('dim');
    cyclePdfAppearance();
    expect(pdfAppearance.mode).toBe('invert');
    cyclePdfAppearance();
    expect(pdfAppearance.mode).toBe('normal');
    expect(localStorage.getItem('xuewen-pdf-appearance')).toBe('normal');
  });

  it('restores the saved mode at init and ignores junk', () => {
    localStorage.setItem('xuewen-pdf-appearance', 'invert');
    initPdfAppearance();
    expect(pdfAppearance.mode).toBe('invert');
    localStorage.setItem('xuewen-pdf-appearance', 'sepia');
    pdfAppearance.mode = 'normal';
    initPdfAppearance();
    expect(pdfAppearance.mode).toBe('normal');
  });
});
