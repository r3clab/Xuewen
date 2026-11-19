import { render } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// The selection→translate wiring under test is all window-level (the settled-
// selection feed + the svelte:window pointer handlers), so PdfPages only needs
// a document scope that renders nothing (the document never opens).
vi.mock('@embedpdf/svelte/runtime', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  DocumentScope: () => {},
  DocumentGate: () => {},
}));

const requestTranslate = vi.fn(async (_text: string, _at: { x: number; y: number }) => {});
vi.mock(import('$features/reader/translation/state.svelte'), async (importOriginal) => ({
  ...(await importOriginal<typeof import('$features/reader/translation/state.svelte')>()),
  requestTranslate: (text: string, at: { x: number; y: number }) => requestTranslate(text, at),
  translateTrigger: () => 'auto' as const,
}));

import PdfPages from './PdfPages.svelte';
import { createPdfCopy, registerPdfCopy, type PdfCopy } from '$features/reader/pdfCopy';
import { appSettings } from '$shared/api/settings.svelte';

/// A `readText` result resolvable by hand — a REAL copier drives the feed, so
/// these tests cover the actual settle/prefetch timing PdfPages parks
/// against, not a re-telling of it.
function deferred<T>(): { promise: Promise<T>; resolve: (v: T) => void } {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

let handlers: { changed?: (ev: { range: unknown }) => void; committed?: () => void };
let pending: ReturnType<typeof deferred<string>>[];
let copier: PdfCopy;

/// Resolve the newest pending read and drain the then-chain behind it (the
/// settled-selection notify sits several microtask hops down).
async function resolvePending(text: string): Promise<void> {
  const job = pending.pop();
  job?.resolve(text);
  await job?.promise;
  for (let i = 0; i < 4; i++) await Promise.resolve();
}

const RANGE = { start: {}, end: {} };

let unregisterCopier: () => void;
beforeEach(() => {
  vi.useFakeTimers();
  handlers = {};
  pending = [];
  copier = createPdfCopy({ activeDocumentId: () => 'a', copy: async () => {} });
  copier.attach('a', {
    readText: () => {
      const d = deferred<string>();
      pending.push(d);
      return d.promise;
    },
    onChanged: (listener) => {
      handlers.changed = listener;
      return () => {};
    },
    onCommitted: (listener) => {
      handlers.committed = listener;
      return () => {};
    },
  });
  unregisterCopier = registerPdfCopy(copier);
  appSettings.translate = { enabled: true, trigger: 'auto' };
  requestTranslate.mockClear();
});

afterEach(() => {
  copier.destroy();
  unregisterCopier();
  appSettings.translate = { enabled: false };
  vi.useRealTimers();
});

describe('PdfPages selection→translate parking', () => {
  it('fires a mid-drag parked settle once, at the release point', async () => {
    // A gutter release: the settle landed during a mid-drag pause, and no
    // commit ever comes — the parked text is the final text.
    render(PdfPages, { props: { documentId: 'a' } });
    window.dispatchEvent(new MouseEvent('pointerdown'));
    handlers.changed?.({ range: RANGE });
    vi.advanceTimersByTime(200);
    await resolvePending('paused text');
    expect(requestTranslate).not.toHaveBeenCalled(); // parked, not fired
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: 40, clientY: 50 }));
    expect(requestTranslate).toHaveBeenCalledTimes(1);
    expect(requestTranslate).toHaveBeenCalledWith('paused text', { x: 40, y: 50 });
  });

  it('drops a stale parked settle when the drag resumed, firing once with the final text', async () => {
    // The double-fire regression: pause mid-drag long enough for the settle
    // read to land (parking partial text), keep dragging, release. The parked
    // partial must NOT fire at release — the pending read announces the final
    // text and fires the one translation itself.
    render(PdfPages, { props: { documentId: 'a' } });
    window.dispatchEvent(new MouseEvent('pointerdown'));
    handlers.changed?.({ range: RANGE });
    vi.advanceTimersByTime(200);
    await resolvePending('partial text');
    handlers.changed?.({ range: RANGE }); // the drag resumes — the parked text is now stale
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: 100, clientY: 120 }));
    expect(requestTranslate).not.toHaveBeenCalled();
    handlers.committed?.(); // the gesture ends with a selection → commit prefetch
    await resolvePending('final full text');
    expect(requestTranslate).toHaveBeenCalledTimes(1);
    expect(requestTranslate).toHaveBeenCalledWith('final full text', { x: 100, y: 120 });
  });

  it('drops parked text when the selection is cleared before the release', async () => {
    render(PdfPages, { props: { documentId: 'a' } });
    window.dispatchEvent(new MouseEvent('pointerdown'));
    handlers.changed?.({ range: RANGE });
    vi.advanceTimersByTime(200);
    await resolvePending('paused text');
    handlers.changed?.({ range: null });
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: 40, clientY: 50 }));
    expect(requestTranslate).not.toHaveBeenCalled();
  });
});
