import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { runWhenIdle } from './idle';

describe('runWhenIdle', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('runs the callback via the setTimeout fallback (jsdom has no requestIdleCallback)', () => {
    const fn = vi.fn();
    runWhenIdle(fn);
    expect(fn).not.toHaveBeenCalled();
    vi.runAllTimers();
    expect(fn).toHaveBeenCalledOnce();
  });

  it('cancel prevents the callback', () => {
    const fn = vi.fn();
    const cancel = runWhenIdle(fn);
    cancel();
    vi.runAllTimers();
    expect(fn).not.toHaveBeenCalled();
  });
});
