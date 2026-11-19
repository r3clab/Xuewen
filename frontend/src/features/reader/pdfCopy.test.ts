import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  attachPdfSelection,
  createPdfCopy,
  onPdfSelectionSettled,
  pdfSelectionFetchPending,
  registerPdfCopy,
  type PdfCopy,
  type SelectionSource,
} from './pdfCopy';

/// A `readText` result resolvable by hand, so a test can hold a fetch in
/// flight and interleave a second selection with it.
function deferred<T>(): { promise: Promise<T>; resolve: (v: T) => void; reject: (e: unknown) => void } {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

const RANGE = { start: {}, end: {} };

function fakeSelection(documentId: string, pending: { documentId: string; d: ReturnType<typeof deferred<string>> }[]) {
  const handlers: { changed?: (ev: { range: unknown }) => void; committed?: () => void } = {};
  const unsubs = { changed: vi.fn(), committed: vi.fn() };
  const readText = vi.fn(() => {
    const d = deferred<string>();
    pending.push({ documentId, d });
    return d.promise;
  });
  const source: SelectionSource = {
    readText,
    onChanged: (listener) => {
      handlers.changed = listener;
      return unsubs.changed;
    },
    onCommitted: (listener) => {
      handlers.committed = listener;
      return unsubs.committed;
    },
  };
  return {
    source,
    readText,
    unsubs,
    change: (range: unknown = RANGE) => handlers.changed?.({ range }),
    commit: () => handlers.committed?.(),
  };
}

function harness() {
  const pending: { documentId: string; d: ReturnType<typeof deferred<string>> }[] = [];
  let activeId: string | null = 'a';
  const copy = vi.fn(async () => {});
  const clearNativeSelection = vi.fn();
  const onError = vi.fn();

  const copier = createPdfCopy({
    activeDocumentId: () => activeId,
    copy,
    clearNativeSelection,
    onError,
  });
  const docs = { a: fakeSelection('a', pending), b: fakeSelection('b', pending) };
  const detach = { a: copier.attach('a', docs.a.source), b: copier.attach('b', docs.b.source) };

  return {
    copier,
    docs,
    detach,
    pending,
    copy,
    clearNativeSelection,
    onError,
    setActive: (id: string | null) => {
      activeId = id;
    },
    /// Finish a drag the way a release on the page does, then settle the one
    /// fetch it kicks off.
    async select(documentId: 'a' | 'b', text: string) {
      docs[documentId].change();
      docs[documentId].commit();
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
  it('caches the text once the selection is committed and copies it', async () => {
    await h.select('a', 'hello world');
    expect(copier.hasSelection()).toBe(true);
    await copier.copySelection();
    expect(h.copy).toHaveBeenCalledWith('hello world');
  });

  it('writes the clipboard synchronously when the text is cached', async () => {
    await h.select('a', 'hello');
    // The WebKit transient-activation invariant: no await may sit between the
    // keystroke and the write. This is the test that stops a refactor from
    // slipping one in.
    void copier.copySelection();
    expect(h.copy).toHaveBeenCalledTimes(1);
  });

  it('fetches after the selection settles even with no commit', async () => {
    h.docs.a.change();
    expect(h.docs.a.readText).not.toHaveBeenCalled();
    expect(copier.hasSelection()).toBe(true); // pending settle still claims ⌘C
    vi.advanceTimersByTime(200);
    const job = h.pending.pop();
    job?.d.resolve('late geometry');
    await job?.d.promise;
    await Promise.resolve();
    await copier.copySelection();
    expect(h.copy).toHaveBeenCalledWith('late geometry');
  });

  it('coalesces a moving selection into one fetch', async () => {
    for (let i = 0; i < 5; i++) {
      h.docs.a.change({ start: {}, end: { i } });
      vi.advanceTimersByTime(50);
    }
    expect(h.docs.a.readText).not.toHaveBeenCalled();
    vi.advanceTimersByTime(200);
    expect(h.docs.a.readText).toHaveBeenCalledTimes(1);
  });

  it('ignores a selection in a background tab', async () => {
    h.docs.b.change();
    h.docs.b.commit();
    vi.advanceTimersByTime(500);
    expect(h.docs.b.readText).not.toHaveBeenCalled();
    expect(copier.hasSelection()).toBe(false);
  });

  it('keeps each tab’s selection so a switch back still copies', async () => {
    await h.select('a', 'from a');
    h.setActive('b');
    await h.select('b', 'from b');
    await copier.copySelection();
    expect(h.copy).toHaveBeenLastCalledWith('from b');
    h.setActive('a');
    expect(copier.hasSelection()).toBe(true);
    await copier.copySelection();
    expect(h.copy).toHaveBeenLastCalledWith('from a');
  });

  it('does not let a stale fetch clobber a newer one on the same tab', async () => {
    h.docs.a.change();
    h.docs.a.commit();
    const first = h.pending.pop();
    h.docs.a.change();
    h.docs.a.commit();
    const second = h.pending.pop();
    second?.d.resolve('newer');
    await second?.d.promise;
    await Promise.resolve();
    first?.d.resolve('older');
    await first?.d.promise;
    await Promise.resolve();
    await copier.copySelection();
    expect(h.copy).toHaveBeenCalledWith('newer');
  });

  it('invalidates on selection change and clear', async () => {
    await h.select('a', 'hello');
    h.docs.a.change(null);
    expect(copier.hasSelection()).toBe(false);
    // hasSelection() is what gates the ⌘C branch, so this never runs in
    // practice; called anyway it re-reads, the plugin reports no selection,
    // and nothing reaches the clipboard.
    const done = copier.copySelection();
    h.pending.pop()?.d.resolve('');
    await done;
    expect(h.copy).not.toHaveBeenCalled();
  });

  it('never puts an empty or blank result on the clipboard', async () => {
    await h.select('a', '');
    expect(copier.hasSelection()).toBe(false);
    await h.select('a', '   ');
    expect(copier.hasSelection()).toBe(false);
    const done = copier.copySelection();
    h.pending.pop()?.d.resolve('   ');
    await done;
    expect(h.copy).not.toHaveBeenCalled();
  });

  it('swallows a rejected fetch without an unhandled rejection', async () => {
    h.docs.a.change();
    h.docs.a.commit();
    const job = h.pending.pop();
    job?.d.reject(new Error('permission-denied'));
    await job?.d.promise.catch(() => {});
    await Promise.resolve();
    await Promise.resolve();
    expect(copier.hasSelection()).toBe(false);
    expect(h.onError).not.toHaveBeenCalled();
  });

  it('awaits an in-flight fetch when ⌘C beats the prefetch', async () => {
    h.docs.a.change();
    h.docs.a.commit();
    const job = h.pending.pop();
    const done = copier.copySelection();
    expect(h.copy).not.toHaveBeenCalled(); // still in flight
    job?.d.resolve('late but real');
    await done;
    expect(h.copy).toHaveBeenCalledWith('late but real');
  });

  it('falls back to a cold fetch when nothing is cached or in flight', async () => {
    const done = copier.copySelection();
    const job = h.pending.pop();
    expect(job?.documentId).toBe('a');
    job?.d.resolve('cold');
    await done;
    expect(h.copy).toHaveBeenCalledWith('cold');
  });

  it('copies nothing for a document no view has attached', async () => {
    h.setActive('c');
    await copier.copySelection();
    expect(h.pending).toEqual([]);
    expect(h.copy).not.toHaveBeenCalled();
  });

  it('surfaces a failed clipboard write through onError', async () => {
    h.copy.mockRejectedValueOnce(new Error('denied'));
    await h.select('a', 'hello');
    await expect(copier.copySelection()).resolves.toBeUndefined();
    expect(h.onError).toHaveBeenCalledWith("Couldn't copy");
  });

  it('clears a stale native selection while a PDF selection is being made', () => {
    h.docs.a.change();
    expect(h.clearNativeSelection).toHaveBeenCalled();
    h.clearNativeSelection.mockClear();
    h.docs.b.change(); // background tab
    h.docs.a.change(null); // a clear is no selection being made
    expect(h.clearNativeSelection).not.toHaveBeenCalled();
  });

  it('forgets a closed document', async () => {
    await h.select('a', 'hello');
    copier.forget('a');
    expect(copier.hasSelection()).toBe(false);
  });

  it('stops following a document once its view detaches', async () => {
    await h.select('a', 'hello');
    h.detach.a();
    expect(h.docs.a.unsubs.changed).toHaveBeenCalled();
    expect(h.docs.a.unsubs.committed).toHaveBeenCalled();
    expect(copier.hasSelection()).toBe(false);
  });

  it('unsubscribes on destroy', async () => {
    await h.select('a', 'hello');
    copier.destroy();
    for (const doc of [h.docs.a, h.docs.b]) {
      expect(doc.unsubs.changed).toHaveBeenCalledTimes(1);
      expect(doc.unsubs.committed).toHaveBeenCalledTimes(1);
    }
    expect(copier.hasSelection()).toBe(false);
  });
});

/// What PdfPages' pointerup consults to drop a stale mid-drag parked settle:
/// pending must cover the whole settle-timer + fetch-in-flight window, and
/// nothing outside it.
describe('fetchPending', () => {
  it('is true from settle-arm through fetch-in-flight, false once the text lands', async () => {
    expect(copier.fetchPending('a')).toBe(false);
    h.docs.a.change();
    expect(copier.fetchPending('a')).toBe(true); // settle timer armed
    vi.advanceTimersByTime(200);
    expect(copier.fetchPending('a')).toBe(true); // fetch in flight
    const job = h.pending.pop();
    job?.d.resolve('text');
    await job?.d.promise;
    for (let i = 0; i < 4; i++) await Promise.resolve();
    expect(copier.fetchPending('a')).toBe(false);
  });

  it('pdfSelectionFetchPending reads the registered copier and is false without one', () => {
    expect(pdfSelectionFetchPending('a')).toBe(false); // no copier registered
    registerPdfCopy(copier);
    h.docs.a.change();
    expect(pdfSelectionFetchPending('a')).toBe(true);
    registerPdfCopy(null);
    expect(pdfSelectionFetchPending('a')).toBe(false);
  });
});

describe('attachPdfSelection', () => {
  afterEach(() => registerPdfCopy(null));

  it('attaches to the registered copier', () => {
    registerPdfCopy(copier);
    const doc = fakeSelection('c', h.pending);
    const detach = attachPdfSelection('c', doc.source);
    h.setActive('c');
    doc.change();
    expect(copier.hasSelection()).toBe(true);
    detach();
    expect(doc.unsubs.changed).toHaveBeenCalled();
    expect(copier.hasSelection()).toBe(false);
  });

  it('is a no-op without a copier', () => {
    const doc = fakeSelection('c', h.pending);
    attachPdfSelection('c', doc.source)();
    expect(doc.unsubs.changed).not.toHaveBeenCalled();
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
  async function resolvePending(text: string): Promise<void> {
    const job = h.pending.pop();
    job?.d.resolve(text);
    await job?.d.promise;
    for (let i = 0; i < 4; i++) await Promise.resolve();
  }

  it('announces the settled text even when no commit ever fires', async () => {
    h.docs.a.change();
    vi.advanceTimersByTime(200);
    await resolvePending('line one\nline two');
    expect(seen).toEqual([['a', 'line one\nline two']]);
  });

  it('does not re-announce identical text when the commit follows a settle', async () => {
    h.docs.a.change();
    vi.advanceTimersByTime(200);
    await resolvePending('same');
    // A drag that paused past the settle before its release: the commit's
    // re-fetch of the same text must not announce a second time (one
    // selection, one translation).
    h.docs.a.commit();
    await resolvePending('same');
    expect(seen).toEqual([['a', 'same']]);
  });

  it('announces a cleared selection so a consumer can dismiss its UI', async () => {
    h.docs.a.change();
    h.docs.a.commit();
    await resolvePending('shown');
    h.docs.a.change(null);
    expect(seen).toEqual([
      ['a', 'shown'],
      ['a', ''],
    ]);
  });
});
