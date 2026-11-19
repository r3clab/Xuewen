import { afterEach, describe, expect, it, vi } from 'vitest';
import { Spring } from 'svelte/motion';
import { DUR, EASE, SPRINGS, dur, prefersReducedMotion, springTo } from './motion';

afterEach(() => vi.unstubAllGlobals());

function stubReducedMotion(matches: boolean): void {
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: query.includes('prefers-reduced-motion') ? matches : false,
    media: query,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

describe('motion tokens', () => {
  it('exposes the shared vocabulary', () => {
    expect(DUR).toEqual({ fast: 150, base: 250, slow: 400 });
    expect(EASE).toBe('cubic-bezier(0.22, 1, 0.36, 1)');
    expect(SPRINGS.pane.stiffness).toBeGreaterThan(0);
  });

  it('prefersReducedMotion reads the media query', () => {
    stubReducedMotion(true);
    expect(prefersReducedMotion()).toBe(true);
    stubReducedMotion(false);
    expect(prefersReducedMotion()).toBe(false);
  });

  it('dur keeps a duration unless motion is reduced', () => {
    stubReducedMotion(false);
    expect(dur(250)).toBe(250);
    stubReducedMotion(true);
    expect(dur(250)).toBe(0);
  });

  it('springTo animates toward the target', () => {
    stubReducedMotion(false);
    const s = new Spring(0);
    springTo(s, 304);
    expect(s.target).toBe(304);
    expect(s.current).toBe(0);
  });

  it('springTo snaps straight to the target under reduced motion', () => {
    stubReducedMotion(true);
    const s = new Spring(0);
    springTo(s, 304);
    expect(s.current).toBe(304);
  });
});
