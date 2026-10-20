import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  annotationTools,
  disarmTools,
  initAnnotationTools,
  setActiveTool,
  setToolColor,
  toggleTool,
} from './state.svelte';

beforeEach(() => {
  localStorage.clear();
  annotationTools.active = null;
  annotationTools.color = 'amber';
});

describe('the armed tool', () => {
  it('starts disarmed', () => {
    expect(annotationTools.active).toBeNull();
  });

  it('toggles the same tool off and swaps between different ones', () => {
    toggleTool('highlight');
    expect(annotationTools.active).toBe('highlight');
    toggleTool('highlight');
    expect(annotationTools.active).toBeNull();
    toggleTool('underline');
    toggleTool('squiggly');
    expect(annotationTools.active).toBe('squiggly');
  });

  it('disarms when the reader goes away', () => {
    setActiveTool('text_comment');
    disarmTools();
    expect(annotationTools.active).toBeNull();
  });

  it('is not persisted — a reload must not leave a highlighter armed', () => {
    setActiveTool('highlight');
    initAnnotationTools();
    expect(annotationTools.active).toBe('highlight'); // untouched in memory
    expect(localStorage.length).toBe(0);
  });
});

describe('the tool color', () => {
  it('persists and comes back on init', () => {
    setToolColor('violet');
    annotationTools.color = 'amber'; // simulate a fresh page
    initAnnotationTools();
    expect(annotationTools.color).toBe('violet');
  });

  it('keeps the default when nothing is stored or the value is junk', () => {
    initAnnotationTools();
    expect(annotationTools.color).toBe('amber');
    localStorage.setItem('xuewen-annotation-color', 'chartreuse');
    initAnnotationTools();
    expect(annotationTools.color).toBe('amber');
  });

  it('still applies the choice when localStorage refuses', () => {
    const setItem = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('quota exceeded');
    });
    expect(() => setToolColor('blue')).not.toThrow();
    expect(annotationTools.color).toBe('blue');
    setItem.mockRestore();
  });

  it('survives a localStorage read that throws', () => {
    const getItem = vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    expect(() => initAnnotationTools()).not.toThrow();
    expect(annotationTools.color).toBe('amber');
    getItem.mockRestore();
  });
});
