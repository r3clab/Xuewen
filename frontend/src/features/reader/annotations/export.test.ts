import { beforeEach, describe, expect, it, vi } from 'vitest';
import { PdfAnnotationSubtype } from '@embedpdf/models';
import type { PdfDocumentObject } from '@embedpdf/models';
import type { AnnotationEvent, AnnotationTransferItem } from '@embedpdf/plugin-annotation';
import {
  buildAnnotatedPdf,
  exportDocumentId,
  exportErrorMessage,
  type ExportDeps,
} from './export';
import { colorHex } from './palette';
import type { Annotation } from '$shared/api/generated/Annotation';

function row(id: string, over: Partial<Annotation> = {}): Annotation {
  return {
    paper_id: 'p1',
    id,
    kind: 'highlight',
    page_index: 1,
    color: 'amber',
    quoted_text: 'quoted',
    note: null,
    payload: {
      annotation: {
        id,
        type: PdfAnnotationSubtype.HIGHLIGHT,
        pageIndex: 1,
        rect: { origin: { x: 0, y: 0 }, size: { width: 10, height: 10 } },
        color: colorHex('amber'),
      },
    },
    created_at: '2026-08-14T00:00:00Z',
    updated_at: '2026-08-14T00:00:00Z',
    ...over,
  };
}

const doc = { id: 'export:p1', pageCount: 3 } as unknown as PdfDocumentObject;

/// A stand-in for the throwaway document: the plugin's `loaded` event fires
/// once the open resolves, the way it does when a PDF's own annotations have
/// been read.
function harness(over: Partial<ExportDeps> = {}) {
  let handler: ((e: AnnotationEvent) => void) | null = null;
  const imported: AnnotationTransferItem[][] = [];
  const closed: string[] = [];
  const order: string[] = [];

  const deps: ExportDeps = {
    open: vi.fn(async () => {
      order.push('open');
      // The plugin reads the document's own marks as the document opens.
      handler?.({ type: 'loaded', documentId: exportDocumentId('p1'), total: 0 });
      return doc;
    }),
    close: vi.fn((id: string) => {
      order.push('close');
      closed.push(id);
    }),
    scope: vi.fn(() => ({
      importAnnotations: (items: AnnotationTransferItem[]) => {
        order.push('import');
        imported.push(items);
      },
      commit: vi.fn(async () => {
        order.push('commit');
        return true;
      }),
      onAnnotationEvent: (h: (e: AnnotationEvent) => void) => {
        handler = h;
        return () => {
          handler = null;
        };
      },
    })),
    save: vi.fn(async () => {
      order.push('save');
      return new Uint8Array([37, 80, 68, 70]).buffer; // "%PDF"
    }),
    timeoutMs: 50,
    ...over,
  };

  return {
    deps,
    imported,
    closed,
    order,
    get listening(): boolean {
      return handler !== null;
    },
    fire(e: AnnotationEvent): void {
      handler?.(e);
    },
  };
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe('exportDocumentId', () => {
  it('cannot collide with the paper’s own document id', () => {
    // Open tabs use the bare paper id as their document id (see PdfDeck);
    // reusing one would hand the export a document someone is reading.
    expect(exportDocumentId('p1')).not.toBe('p1');
  });
});

describe('buildAnnotatedPdf', () => {
  it('replays the marks into a throwaway document and saves that', async () => {
    const h = harness();
    const blob = await buildAnnotatedPdf('p1', [row('a1'), row('a2')], h.deps);
    expect(blob.type).toBe('application/pdf');
    expect(h.imported[0].map((i) => i.annotation.id)).toEqual(['a1', 'a2']);
    // Never the paper's own document: the open document must stay untouched,
    // which is the whole reason for a second one.
    expect(h.deps.open).toHaveBeenCalledWith(exportDocumentId('p1'));
    expect(h.deps.save).toHaveBeenCalledWith(doc);
  });

  it('commits after the import and before the save', async () => {
    const h = harness();
    await buildAnnotatedPdf('p1', [row('a1')], h.deps);
    // Committing early would write an empty copy; saving early would write one
    // without the marks.
    expect(h.order).toEqual(['open', 'import', 'commit', 'save', 'close']);
  });

  it('closes the throwaway and stops listening once it is done', async () => {
    const h = harness();
    await buildAnnotatedPdf('p1', [row('a1')], h.deps);
    expect(h.closed).toEqual([exportDocumentId('p1')]);
    expect(h.listening).toBe(false);
  });

  it('closes the throwaway even when the save fails', async () => {
    const h = harness({
      save: vi.fn(async () => {
        throw { code: 3, message: 'saving failed' };
      }),
    });
    await expect(buildAnnotatedPdf('p1', [row('a1')], h.deps)).rejects.toBeDefined();
    // A leaked document would hold a PDFium handle and count against the
    // manager's open-document cap for the rest of the session.
    expect(h.closed).toEqual([exportDocumentId('p1')]);
    expect(h.listening).toBe(false);
  });

  it('closes the throwaway when the document never opens', async () => {
    const h = harness({
      open: vi.fn(async () => {
        throw new Error('404');
      }),
    });
    await expect(buildAnnotatedPdf('p1', [row('a1')], h.deps)).rejects.toThrow('404');
    expect(h.closed).toEqual([exportDocumentId('p1')]);
  });

  it('gives up rather than hanging when the document never finishes loading', async () => {
    const h = harness({ open: vi.fn(async () => doc), timeoutMs: 10 });
    await expect(buildAnnotatedPdf('p1', [row('a1')], h.deps)).rejects.toThrow(
      /did not finish loading/,
    );
    expect(h.closed).toEqual([exportDocumentId('p1')]);
  });

  it('waits for a load that arrives after the open resolves', async () => {
    const h = harness({ open: vi.fn(async () => doc) });
    const done = buildAnnotatedPdf('p1', [row('a1')], h.deps);
    await Promise.resolve(); // let the import happen
    h.fire({ type: 'loaded', documentId: exportDocumentId('p1'), total: 0 });
    await expect(done).resolves.toBeInstanceOf(Blob);
  });

  it('skips a row whose payload cannot be rebuilt rather than failing', async () => {
    const h = harness();
    await buildAnnotatedPdf('p1', [row('a1'), row('bad', { payload: null })], h.deps);
    expect(h.imported[0].map((i) => i.annotation.id)).toEqual(['a1']);
  });
});

describe('exportErrorMessage', () => {
  it('reads the engine’s plain {code, message} reason', () => {
    // PDFium failures are not Errors, so `.message` is all there is to go on.
    expect(exportErrorMessage({ code: 3, message: 'password required' })).toBe('password required');
  });

  it('falls back to something a reader can act on', () => {
    expect(exportErrorMessage(null)).toMatch(/exporting the annotated PDF failed/);
    expect(exportErrorMessage({ code: 3 })).toMatch(/exporting the annotated PDF failed/);
  });
});
