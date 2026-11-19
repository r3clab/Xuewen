import { describe, expect, it } from 'vitest';
import { aboveAnchor, clampMenuPosition } from './popoverPosition';

const menu = { offsetWidth: 100, offsetHeight: 50 };

describe('clampMenuPosition', () => {
  it('leaves a position alone when the menu fits', () => {
    expect(clampMenuPosition(10, 20, menu)).toEqual({ left: 10, top: 20 });
  });

  it('clamps to the right/bottom viewport edges with an 8px margin', () => {
    const { left, top } = clampMenuPosition(1e6, 1e6, menu);
    expect(left).toBe(window.innerWidth - menu.offsetWidth - 8);
    expect(top).toBe(window.innerHeight - menu.offsetHeight - 8);
  });
});

describe('aboveAnchor', () => {
  function popover(width: number, height: number): HTMLElement {
    const el = document.createElement('div');
    Object.defineProperty(el, 'offsetWidth', { value: width });
    Object.defineProperty(el, 'offsetHeight', { value: height });
    return el;
  }

  it('sits above the point when its measured height fits', () => {
    const el = popover(320, 120);
    aboveAnchor(100, 300, 12)(el);
    expect(el.style.top).toBe(`${300 - 8 - 120}px`);
    expect(el.style.left).toBe('100px');
  });

  it('flips below the point when it would clip the top edge', () => {
    const el = popover(320, 120);
    aboveAnchor(100, 100, 12)(el);
    expect(el.style.top).toBe('112px');
  });

  it('clamps to the left and right viewport edges', () => {
    const wide = popover(320, 10);
    aboveAnchor(1e6, 300, 12)(wide);
    expect(wide.style.left).toBe(`${window.innerWidth - 320 - 8}px`);
    const nudged = popover(320, 10);
    aboveAnchor(-50, 300, 12)(nudged);
    expect(nudged.style.left).toBe('8px');
  });
});
