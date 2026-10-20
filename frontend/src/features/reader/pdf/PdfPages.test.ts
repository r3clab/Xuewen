import { render } from '@testing-library/svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// svelte/motion (Spring) constructs a prefers-reduced-motion MediaQuery at
// module load, and jsdom has no matchMedia — hoisted so the stub exists before
// PdfPages' imports run.
vi.hoisted(() => {
  window.matchMedia ??= ((query: string) =>
    ({
      matches: false,
      media: query,
      addEventListener: () => {},
      removeEventListener: () => {},
    }) as unknown as MediaQueryList) as typeof window.matchMedia;
});

// The selection→translate wiring under test is all window-level (the settled-
// selection feed + the svelte:window pointer handlers), so PdfPages only needs
// enough EmbedPDF surface to mount: hooks that take their early-return paths
// and a DocumentContent that renders nothing (the document never loads).
vi.mock('@embedpdf/core/svelte', () => ({
  useRegistry: () => ({ registry: null }),
  useDocumentState: () => ({ current: null }),
  // What useViewportCapability (scroll-hide wiring) resolves through.
  useCapability: () => ({ provides: null, isLoading: false, ready: Promise.resolve() }),
}));
// Partial: annotations/renderers.ts needs the real createRenderer at import.
vi.mock('@embedpdf/plugin-annotation/svelte', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@embedpdf/plugin-annotation/svelte')>()),
  useAnnotation: () => ({ provides: null }),
  useAnnotationCapability: () => ({ provides: null }),
}));
vi.mock('@embedpdf/plugin-document-manager/svelte', () => ({
  DocumentContent: () => {},
}));

const requestTranslate = vi.fn(async (_text: string, _at: { x: number; y: number }) => {});
vi.mock(import('$features/reader/translation/state.svelte'), async (importOriginal) => ({
  ...(await importOriginal<typeof import('$features/reader/translation/state.svelte')>()),
  requestTranslate: (text: string, at: { x: number; y: number }) => requestTranslate(text, at),
  translateTrigger: () => 'auto' as const,
}));

import PdfPages from './PdfPages.svelte';
import { createPdfCopy, registerPdfCopy, type PdfCopy, type SelectionLike } from '$features/reader/pdfCopy';
import { appSettings } from '$shared/api/settings.svelte';

/// The same hand-resolvable stand-in for `getSelectedText`'s PdfTask that
/// pdfCopy.test.ts uses — a REAL copier drives the feed, so these tests cover
/// the actual settle/prefetch timing PdfPages parks against, not a re-telling
/// of it.
function deferred<T>(): { promise: Promise<T>; resolve: (v: T) => void } {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

let handlers: {
  change?: (ev: { documentId: string; selection: unknown }) => void;
  end?: (ev: { documentId: string }) => void;
};
let pending: ReturnType<typeof deferred<string[]>>[];
let copier: PdfCopy;

/// Resolve the newest pending fetch and drain the then-chain behind it (the
/// settled-selection notify sits several microtask hops down).
async function resolvePending(text: string[]): Promise<void> {
  const job = pending.pop();
  job?.resolve(text);
  await job?.promise;
  for (let i = 0; i < 4; i++) await Promise.resolve();
}

const drag = { documentId: 'a', selection: { start: {}, end: {} } };

beforeEach(() => {
  vi.useFakeTimers();
  handlers = {};
  pending = [];
  const selection: SelectionLike = {
    getSelectedText: () => {
      const d = deferred<string[]>();
      pending.push(d);
      return { toPromise: () => d.promise };
    },
    onBeginSelection: () => () => {},
    onSelectionChange: (h) => {
      handlers.change = h;
      return () => {};
    },
    onEndSelection: (h) => {
      handlers.end = h;
      return () => {};
    },
  };
  copier = createPdfCopy({ selection, activeDocumentId: () => 'a', copy: async () => {} });
  registerPdfCopy(copier);
  appSettings.translate = { enabled: true, trigger: 'auto' };
  requestTranslate.mockClear();
});

afterEach(() => {
  copier.destroy();
  registerPdfCopy(null);
  appSettings.translate = { enabled: false };
  vi.useRealTimers();
});

describe('PdfPages selection→translate parking', () => {
  it('fires a mid-drag parked settle once, at the release point', async () => {
    // A gutter release: the settle landed during a mid-drag pause, and no
    // onEndSelection ever comes — the parked text is the final text.
    render(PdfPages, { props: { documentId: 'a' } });
    window.dispatchEvent(new MouseEvent('pointerdown'));
    handlers.change?.(drag);
    vi.advanceTimersByTime(200);
    await resolvePending(['paused text']);
    expect(requestTranslate).not.toHaveBeenCalled(); // parked, not fired
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: 40, clientY: 50 }));
    expect(requestTranslate).toHaveBeenCalledTimes(1);
    expect(requestTranslate).toHaveBeenCalledWith('paused text', { x: 40, y: 50 });
  });

  it('drops a stale parked settle when the drag resumed, firing once with the final text', async () => {
    // The double-fire regression: pause mid-drag long enough for the settle
    // fetch to land (parking partial text), keep dragging, release. The parked
    // partial must NOT fire at release — the pending fetch announces the final
    // text and fires the one translation itself.
    render(PdfPages, { props: { documentId: 'a' } });
    window.dispatchEvent(new MouseEvent('pointerdown'));
    handlers.change?.(drag);
    vi.advanceTimersByTime(200);
    await resolvePending(['partial text']);
    handlers.change?.(drag); // the drag resumes — the parked text is now stale
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: 100, clientY: 120 }));
    expect(requestTranslate).not.toHaveBeenCalled();
    handlers.end?.({ documentId: 'a' }); // release on the page → end prefetch
    await resolvePending(['final full text']);
    expect(requestTranslate).toHaveBeenCalledTimes(1);
    expect(requestTranslate).toHaveBeenCalledWith('final full text', { x: 100, y: 120 });
  });
});
