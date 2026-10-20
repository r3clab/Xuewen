import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { dismissToast, toast, toasts } from './toasts.svelte';

beforeEach(() => {
  vi.useFakeTimers();
  toasts.items.length = 0;
});
afterEach(() => vi.useRealTimers());

describe('toast store', () => {
  it('pushes and auto-dismisses after the timeout', () => {
    toast('success', 'Citation copied');
    expect(toasts.items).toHaveLength(1);
    expect(toasts.items[0]).toMatchObject({ kind: 'success', message: 'Citation copied' });
    vi.advanceTimersByTime(3500);
    expect(toasts.items).toHaveLength(0);
  });

  it('carries an optional action', () => {
    const run = vi.fn();
    toast('success', 'Paper deleted', 0, { label: 'Undo', run });
    expect(toasts.items[0].action?.label).toBe('Undo');
    toasts.items[0].action?.run();
    expect(run).toHaveBeenCalledOnce();
  });

  it('timeoutMs 0 sticks until dismissed by hand', () => {
    const id = toast('error', 'Import failed', 0);
    vi.advanceTimersByTime(60_000);
    expect(toasts.items).toHaveLength(1);
    dismissToast(id);
    expect(toasts.items).toHaveLength(0);
  });

  it('dismissing an unknown id is a no-op', () => {
    toast('info', 'hello');
    dismissToast(999);
    expect(toasts.items).toHaveLength(1);
  });
});
