import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  createPdfCopy,
  onPdfSelectionSettled,
  pdfSelectionFetchPending,
  registerPdfCopy,
  type PdfCopy,
  type SelectionLike,
} from './pdfCopy';

/// A controllable stand-in for the PdfTask `getSelectedText` returns — the same
/// `{ toPromise }` idiom citations/load.test.ts uses, but resolvable by hand so
/// a test can hold a fetch in flight and interleave a second selection with it.
function deferred<T>(): { promise: Promise<T>; resolve: (v: T) => void; reject: (e: unknown) => void } {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function harness() {
  const handlers: {
    begin?: (ev: { documentId: string }) => void;
    change?: (ev: { documentId: string; selection: unknown }) => void;
    end?: (ev: { documentId: string }) => void;
  } = {};
  const unsubs = { begin: vi.fn(), change: vi.fn(), end: vi.fn() };
  /// Queued per call so a test can stage several fetches; falls back to an
  /// immediately-resolving empty result once the queue is drained.
  const pending: { documentId: string; d: ReturnType<typeof deferred<string[]>> }[] = [];
  const getSelectedText = vi.fn((documentId: string) => {
    const d = deferred<string[]>();
    pending.push({ documentId, d });
    return { toPromise: () => d.promise };
  });

  const selection: SelectionLike = {
    getSelectedText,
    onBeginSelection: (h) => {
      handlers.begin = h;
      return unsubs.begin;
    },
    onSelectionChange: (h) => {
      handlers.change = h;
      return unsubs.change;
    },
    onEndSelection: (h) => {
      handlers.end = h;
      return unsubs.end;
    },
  };

  let activeId: string | null = 'a';
  const copy = vi.fn(async () => {});
  const clearNativeSelection = vi.fn();
  const onError = vi.fn();

  const copier = createPdfCopy({
    selection,
    activeDocumentId: () => activeId,
    copy,
    clearNativeSelection,
    onError,
  });

  return {
    copier,
    handlers,
    unsubs,
    pending,
    getSelectedText,
    copy,
    clearNativeSelection,
    onError,
    setActive: (id: string | null) => {
      activeId = id;
    },
    /// Finish the drag the way a release-on-page drag does, then settle the
    /// one fetch it kicks off.
    async select(documentId: string, text: string[]) {
      handlers.change?.({ documentId, selection: { start: {}, end: {} } });
      handlers.end?.({ documentId });
      const job = pending.pop();
      job?.d.resolve(text);
      await job?.d.promise.catch(() => {});
      await Promise.resolve();
    },
  };
}

let h: ReturnType<typeof harness>;
let copier: PdfCopy;

beforeEach(() => {
  vi.useFakeTimers();
  h = harness();
  copier = h.copier;
});

afterEach(() => {
  copier.destroy();
  vi.useRealTimers();
});

describe('createPdfCopy', () => {
  it('caches the text at selection-end and copies it', async () => {
    await h.select('a', ['hello world']);
    expect(copier.hasSelection()).toBe(true);
    await copier.copySelection();
    expect(h.copy).toHaveBeenCalledWith('hello world');
  });

  it('joins a multi-page selection with newlines', async () => {
    await h.select('a', ['page one', 'page two']);
    await copier.copySelection();
    expect(h.copy).toHaveBeenCalledWith('page one\npage two');
  });

  it('writes the clipboard synchronously when the text is cached', async () => {
    await h.select('a', ['hello']);
    // The WebKit transient-activation invariant: no await may sit between the
    // keystroke and the write. This is the test that stops a refactor from
    // slipping one in.
    void copier.copySelection();
    expect(h.copy).toHaveBeenCalledTimes(1);
  });

  it('fetches after the selection settles even with no end event', async () => {
    // A drag released in the gutter, past the margin, or on another page: the
    // plugin's per-page pointerup never fires, so onEndSelection never comes.
    h.handlers.change?.({ documentId: 'a', selection: { start: {}, end: {} } });
    expect(h.getSelectedText).not.toHaveBeenCalled();
    expect(copier.hasSelection()).toBe(true); // pending settle still claims ⌘C
    vi.advanceTimersByTime(200);
    const job = h.pending.pop();
    job?.d.resolve(['stranded drag']);
    await job?.d.promise;
    await Promise.resolve();
    await copier.copySelection();
    expect(h.copy).toHaveBeenCalledWith('stranded drag');
  });

  it('coalesces a moving selection into one fetch', async () => {
    for (let i = 0; i < 5; i++) {
      h.handlers.change?.({ documentId: 'a', selection: { start: {}, end: { i } } });
      vi.advanceTimersByTime(50);
    }
    expect(h.getSelectedText).not.toHaveBeenCalled();
    vi.advanceTimersByTime(200);
    expect(h.getSelectedText).toHaveBeenCalledTimes(1);
  });

  it('ignores a selection in a background tab', async () => {
    h.handlers.change?.({ documentId: 'b', selection: { start: {}, end: {} } });
    h.handlers.end?.({ documentId: 'b' });
    vi.advanceTimersByTime(500);
    expect(h.getSelectedText).not.toHaveBeenCalled();
    expect(copier.hasSelection()).toBe(false);
  });

  it('keeps each tab’s selection so a switch back still copies', async () => {
    await h.select('a', ['from a']);
    h.setActive('b');
    await h.select('b', ['from b']);
    await copier.copySelection();
    expect(h.copy).toHaveBeenLastCalledWith('from b');
    h.setActive('a');
    expect(copier.hasSelection()).toBe(true);
    await copier.copySelection();
    expect(h.copy).toHaveBeenLastCalledWith('from a');
  });

  it('does not let a stale fetch clobber a newer one on the same tab', async () => {
    h.handlers.change?.({ documentId: 'a', selection: {} });
    h.handlers.end?.({ documentId: 'a' });
    const first = h.pending.pop();
    h.handlers.change?.({ documentId: 'a', selection: {} });
    h.handlers.end?.({ documentId: 'a' });
    const second = h.pending.pop();
    second?.d.resolve(['newer']);
    await second?.d.promise;
    await Promise.resolve();
    first?.d.resolve(['older']);
    await first?.d.promise;
    await Promise.resolve();
    await copier.copySelection();
    expect(h.copy).toHaveBeenCalledWith('newer');
  });

  it('invalidates on selection change and clear', async () => {
    await h.select('a', ['hello']);
    h.handlers.change?.({ documentId: 'a', selection: null });
    expect(copier.hasSelection()).toBe(false);
    // hasSelection() is what gates the ⌘C branch, so this never runs in
    // practice; called anyway it re-checks with the plugin, which reports no
    // selection, and nothing reaches the clipboard.
    const done = copier.copySelection();
    h.pending.pop()?.d.resolve([]);
    await done;
    expect(h.copy).not.toHaveBeenCalled();
  });

  it('never puts an empty or blank result on the clipboard', async () => {
    // Upstream's own copyToClipboard writes `[].join('\n')` here, clobbering
    // whatever the user had on the clipboard with an empty string.
    await h.select('a', []);
    expect(copier.hasSelection()).toBe(false);
    await h.select('a', ['   ']);
    expect(copier.hasSelection()).toBe(false);
    const done = copier.copySelection();
    h.pending.pop()?.d.resolve(['   ']);
    await done;
    expect(h.copy).not.toHaveBeenCalled();
  });

  it('swallows a rejected fetch without an unhandled rejection', async () => {
    h.handlers.change?.({ documentId: 'a', selection: {} });
    h.handlers.end?.({ documentId: 'a' });
    const job = h.pending.pop();
    job?.d.reject(new Error('Doc Not Found or No Selection'));
    await job?.d.promise.catch(() => {});
    await Promise.resolve();
    await Promise.resolve();
    expect(copier.hasSelection()).toBe(false);
    expect(h.onError).not.toHaveBeenCalled();
  });

  it('awaits an in-flight fetch when ⌘C beats the prefetch', async () => {
    h.handlers.change?.({ documentId: 'a', selection: {} });
    h.handlers.end?.({ documentId: 'a' });
    const job = h.pending.pop();
    const done = copier.copySelection();
    expect(h.copy).not.toHaveBeenCalled(); // still in flight
    job?.d.resolve(['late but real']);
    await done;
    expect(h.copy).toHaveBeenCalledWith('late but real');
  });

  it('falls back to a cold fetch when nothing is cached or in flight', async () => {
    const done = copier.copySelection();
    const job = h.pending.pop();
    expect(job?.documentId).toBe('a');
    job?.d.resolve(['cold']);
    await done;
    expect(h.copy).toHaveBeenCalledWith('cold');
  });

  it('surfaces a failed clipboard write through onError', async () => {
    h.copy.mockRejectedValueOnce(new Error('denied'));
    await h.select('a', ['hello']);
    await expect(copier.copySelection()).resolves.toBeUndefined();
    expect(h.onError).toHaveBeenCalledWith("Couldn't copy");
  });

  it('clears a stale native selection when a PDF selection begins', () => {
    h.handlers.begin?.({ documentId: 'a' });
    expect(h.clearNativeSelection).toHaveBeenCalled();
    h.clearNativeSelection.mockClear();
    h.handlers.begin?.({ documentId: 'b' }); // background tab
    expect(h.clearNativeSelection).not.toHaveBeenCalled();
  });

  it('forgets a closed document', async () => {
    await h.select('a', ['hello']);
    copier.forget('a');
    expect(copier.hasSelection()).toBe(false);
  });

  it('unsubscribes on destroy', async () => {
    await h.select('a', ['hello']);
    copier.destroy();
    expect(h.unsubs.begin).toHaveBeenCalled();
    expect(h.unsubs.change).toHaveBeenCalled();
    expect(h.unsubs.end).toHaveBeenCalled();
    expect(copier.hasSelection()).toBe(false);
  });
});

/// What PdfPages' pointerup consults to drop a stale mid-drag parked settle:
/// pending must cover the whole settle-timer + fetch-in-flight window, and
/// nothing outside it.
describe('fetchPending', () => {
  it('is true from settle-arm through fetch-in-flight, false once the text lands', async () => {
    expect(copier.fetchPending('a')).toBe(false);
    h.handlers.change?.({ documentId: 'a', selection: { start: {}, end: {} } });
    expect(copier.fetchPending('a')).toBe(true); // settle timer armed
    vi.advanceTimersByTime(200);
    expect(copier.fetchPending('a')).toBe(true); // fetch in flight
    const job = h.pending.pop();
    job?.d.resolve(['text']);
    await job?.d.promise;
    for (let i = 0; i < 4; i++) await Promise.resolve();
    expect(copier.fetchPending('a')).toBe(false);
  });

  it('pdfSelectionFetchPending reads the registered copier and is false without one', () => {
    expect(pdfSelectionFetchPending('a')).toBe(false); // no copier registered
    registerPdfCopy(copier);
    h.handlers.change?.({ documentId: 'a', selection: { start: {}, end: {} } });
    expect(pdfSelectionFetchPending('a')).toBe(true);
    registerPdfCopy(null);
    expect(pdfSelectionFetchPending('a')).toBe(false);
  });
});

describe('onPdfSelectionSettled', () => {
  let seen: [string, string][];
  let off: () => void;

  beforeEach(() => {
    seen = [];
    off = onPdfSelectionSettled((id, text) => seen.push([id, text]));
  });

  afterEach(() => off());

  /// Resolve a pending fetch and drain the whole then-chain behind it (the
  /// notify sits several hops down), which one `await` would not reach.
  async function resolvePending(text: string[]): Promise<void> {
    const job = h.pending.pop();
    job?.d.resolve(text);
    await job?.d.promise;
    for (let i = 0; i < 4; i++) await Promise.resolve();
  }

  it('announces the settled text even when no end event ever fires', async () => {
    // The gutter-release case translate depends on: a live selection, no
    // onEndSelection, only the settle timer.
    h.handlers.change?.({ documentId: 'a', selection: { start: {}, end: {} } });
    vi.advanceTimersByTime(200);
    await resolvePending(['line one', 'line two']);
    expect(seen).toEqual([['a', 'line one\nline two']]);
  });

  it('does not re-announce identical text when the end event follows a settle', async () => {
    h.handlers.change?.({ documentId: 'a', selection: { start: {}, end: {} } });
    vi.advanceTimersByTime(200);
    await resolvePending(['same']);
    // The release-on-page drag now fires onEndSelection too; its re-fetch of
    // the same text must not announce a second time (one selection, one
    // translation).
    h.handlers.end?.({ documentId: 'a' });
    await resolvePending(['same']);
    expect(seen).toEqual([['a', 'same']]);
  });

  it('announces an empty settle so a consumer can dismiss its UI', async () => {
    h.handlers.change?.({ documentId: 'a', selection: { start: {}, end: {} } });
    h.handlers.end?.({ documentId: 'a' });
    await resolvePending(['shown']);
    // A click on the page: the change clears the cache, then the end-driven
    // fetch comes back empty.
    h.handlers.change?.({ documentId: 'a', selection: null });
    h.handlers.end?.({ documentId: 'a' });
    await resolvePending([]);
    expect(seen).toEqual([
      ['a', 'shown'],
      ['a', ''],
    ]);
  });
});
