import { describe, expect, it } from 'vitest';
import { clampMenuPosition } from './popoverPosition';

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
