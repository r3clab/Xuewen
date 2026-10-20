import { describe, expect, it, vi } from 'vitest';
import type { PdfDocumentObject } from '@embedpdf/models';
import {
  DocumentOpenError,
  documentsToAdopt,
  openDocumentFully,
  planOpens,
  reconcileDocuments,
  type DocumentOpenerLike,
} from './deck';

describe('documentsToAdopt', () => {
  // The startup remount: <EmbedPDF> swaps the branch its children render in
  // once the plugins are ready, so the replacement PdfDeck starts with an empty
  // `opened` while the registry still holds the document the first one opened.
  it('adopts a tab the registry already holds but this deck has forgotten', () => {
    expect(documentsToAdopt([], ['a'], ['a'])).toEqual(['a']);
  });

  it('adopts nothing it already knows about', () => {
    expect(documentsToAdopt(['a'], ['a'], ['a'])).toEqual([]);
  });

  // Adopting it would put it in `opened`, where the next reconcile finds no tab
  // for it and closes it — mid-export.
  it('leaves the export’s throwaway document alone', () => {
    expect(documentsToAdopt([], ['a'], ['a', 'export:a'])).toEqual(['a']);
  });

  it('adopts nothing when the registry is empty', () => {
    expect(documentsToAdopt([], ['a', 'b'], [])).toEqual([]);
  });

  it('ignores a registry document whose tab has already gone', () => {
    expect(documentsToAdopt([], ['a'], ['a', 'b'])).toEqual(['a']);
  });
});

describe('reconcileDocuments', () => {
  it('opens new tabs and closes removed ones', () => {
    const { toOpen, toClose } = reconcileDocuments(['a', 'b'], ['b', 'c']);
    expect(toOpen).toEqual(['c']);
    expect(toClose).toEqual(['a']);
  });

  it('is a no-op when opened matches the tabs exactly', () => {
    const { toOpen, toClose } = reconcileDocuments(['a', 'b'], ['a', 'b']);
    expect(toOpen).toEqual([]);
    expect(toClose).toEqual([]);
  });

  it('opens everything when nothing is opened yet', () => {
    const { toOpen, toClose } = reconcileDocuments([], ['a', 'b', 'c']);
    expect(toOpen).toEqual(['a', 'b', 'c']);
    expect(toClose).toEqual([]);
  });

  it('closes everything when there are no tabs left', () => {
    const { toOpen, toClose } = reconcileDocuments(['a', 'b'], []);
    expect(toOpen).toEqual([]);
    expect(toClose).toEqual(['a', 'b']);
  });
});

describe('planOpens', () => {
  it('opens the tab on screen first and defers the rest', () => {
    // The restored-session case: four tabs, one of them visible.
    const { now, deferred } = planOpens(['a', 'b', 'c', 'd'], 'c');
    expect(now).toEqual(['c']);
    expect(deferred).toEqual(['a', 'b', 'd']);
  });

  it('keeps the deferred tabs in tab order', () => {
    const { deferred } = planOpens(['a', 'b', 'c'], 'a');
    expect(deferred).toEqual(['b', 'c']);
  });

  it('defers everything when the active document is already open', () => {
    // `toOpen` excludes it, so there is nothing on screen left to prioritise —
    // the queued tabs must still drain rather than stall forever.
    const { now, deferred } = planOpens(['b', 'c'], 'a');
    expect(now).toEqual([]);
    expect(deferred).toEqual(['b', 'c']);
  });

  it('defers everything when no tab is active', () => {
    const { now, deferred } = planOpens(['a', 'b'], null);
    expect(now).toEqual([]);
    expect(deferred).toEqual(['a', 'b']);
  });

  it('opening a single paper is unaffected', () => {
    // The common case — one click, one document, opened immediately.
    const { now, deferred } = planOpens(['a'], 'a');
    expect(now).toEqual(['a']);
    expect(deferred).toEqual([]);
  });

  it('never lists a document in both halves', () => {
    const { now, deferred } = planOpens(['a', 'b', 'c'], 'b');
    expect(now.filter((id) => deferred.includes(id))).toEqual([]);
    expect([...now, ...deferred].sort()).toEqual(['a', 'b', 'c']);
  });
});

describe('openDocumentFully', () => {
  const doc = { id: 'd1' } as unknown as PdfDocumentObject;

  function opener(
    outer: () => Promise<{ task: { toPromise(): Promise<PdfDocumentObject> } }>,
  ): DocumentOpenerLike {
    return { openDocumentUrl: vi.fn(() => ({ toPromise: outer })) };
  }

  it('resolves only with the INNER task — the outer one lies about loading', async () => {
    // The outer task resolves synchronously with the id; the inner one is the
    // actual parse. A helper that resolved on the outer task would report
    // "loaded" before a byte was read.
    let innerResolved = false;
    const cap = opener(async () => ({
      task: {
        toPromise: async () => {
          innerResolved = true;
          return doc;
        },
      },
    }));
    await expect(openDocumentFully(cap, { url: '/x.pdf', documentId: 'd1' })).resolves.toBe(doc);
    expect(innerResolved).toBe(true);
    expect(cap.openDocumentUrl).toHaveBeenCalledWith({
      url: '/x.pdf',
      documentId: 'd1',
      autoActivate: false,
    });
  });

  it("rejects with phase 'open' when the outer task fails (cap hit)", async () => {
    const cap = opener(() => Promise.reject({ code: 1, message: 'max documents reached' }));
    const err: unknown = await openDocumentFully(cap, { url: '/x.pdf', documentId: 'd1' }).catch(
      (e: unknown) => e,
    );
    expect(err).toBeInstanceOf(DocumentOpenError);
    expect((err as DocumentOpenError).phase).toBe('open');
    // PdfErrorReason is { code, message }, not an Error — the message must
    // still surface (PdfAnnotations toasts it via exportErrorMessage).
    expect((err as DocumentOpenError).message).toBe('max documents reached');
  });

  it("rejects with phase 'load' when the document itself fails to parse", async () => {
    const cap = opener(async () => ({
      task: { toPromise: () => Promise.reject(new Error('bad xref')) },
    }));
    const err: unknown = await openDocumentFully(cap, { url: '/x.pdf', documentId: 'd1' }).catch(
      (e: unknown) => e,
    );
    expect(err).toBeInstanceOf(DocumentOpenError);
    expect((err as DocumentOpenError).phase).toBe('load');
    expect((err as DocumentOpenError).message).toBe('bad xref');
  });

  it('falls back to a phase-named message when the failure carries none', async () => {
    const cap = opener(() => Promise.reject({}));
    const err: unknown = await openDocumentFully(cap, { url: '/x.pdf', documentId: 'd1' }).catch(
      (e: unknown) => e,
    );
    expect((err as DocumentOpenError).message).toBe('document open failed');
  });
});
