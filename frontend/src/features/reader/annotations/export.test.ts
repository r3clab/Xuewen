import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { AnnotationBundle } from '@embedpdf/svelte/annotation';
import { buildAnnotatedPdf, exportDocumentId, type ExportDeps } from './export';
import { RESTORE_OPTIONS, type RestorePage } from './adapter';
import { colorHex } from './palette';
import type { Annotation } from '$shared/api/generated/Annotation';

const PAGE: RestorePage = { index: 1, ref: { kind: 'objectNumber', objectNumber: 101 }, size: { width: 612, height: 792 } };
const FIRST: RestorePage = { index: 0, ref: { kind: 'objectNumber', objectNumber: 100 }, size: PAGE.size };
const QUAD = {
  upperLeft: { x: 0, y: 0 },
  upperRight: { x: 10, y: 0 },
  lowerLeft: { x: 0, y: 10 },
  lowerRight: { x: 10, y: 10 },
};

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
      data: {
        subtype: 'highlight',
        ref: { kind: 'objectNumber', page: PAGE.ref, objectNumber: 9 },
        nm: id,
        color: colorHex('amber'),
        quadPoints: [QUAD],
      },
    },
    created_at: '2026-08-14T00:00:00Z',
    updated_at: '2026-08-14T00:00:00Z',
    ...over,
  };
}

const PDF = new Uint8Array([37, 80, 68, 70]); // "%PDF"

/// A stand-in for the throwaway document, recording the order things happen
/// to it.
function harness(over: Partial<ExportDeps> = {}) {
  const order: string[] = [];
  const imported: [AnnotationBundle, typeof RESTORE_OPTIONS][] = [];
  const created: unknown[] = [];
  const deps: ExportDeps = {
    open: vi.fn(async () => void order.push('open')),
    close: vi.fn(async () => void order.push('close')),
    pages: vi.fn(() => [FIRST, PAGE]),
    scope: vi.fn(() => ({
      import: async (bundle: AnnotationBundle, options: typeof RESTORE_OPTIONS) => {
        order.push('import');
        imported.push([bundle, options]);
      },
      create: async (page: unknown, draft: unknown) => {
        order.push('create');
        created.push([page, draft]);
      },
    })),
    download: vi.fn(async () => {
      order.push('download');
      return PDF;
    }),
    ...over,
  };
  return { deps, order, imported, created };
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
  it('restores the marks into a throwaway document and downloads that', async () => {
    const h = harness();
    const blob = await buildAnnotatedPdf('p1', [row('a1'), row('a2')], h.deps);
    expect(blob.type).toBe('application/pdf');
    expect(new Uint8Array(await blob.arrayBuffer())).toEqual(PDF);
    // Never the paper's own document: the open document must stay untouched,
    // which is the whole reason for a second one.
    expect(h.deps.open).toHaveBeenCalledWith(exportDocumentId('p1'));
    expect(h.deps.download).toHaveBeenCalledWith(exportDocumentId('p1'));
    const [[bundle, options]] = h.imported;
    expect(options).toEqual(RESTORE_OPTIONS);
    expect(bundle.items.map((item) => item.data.nm)).toEqual(['a1', 'a2']);
    expect(bundle.pages).toEqual([{ page: PAGE.ref, position: 1, size: PAGE.size }]);
  });

  it('downloads only once the marks are in, then closes the copy', async () => {
    const h = harness();
    const rect = { origin: { x: 0, y: 0 }, size: { width: 10, height: 10 } };
    const legacy = row('old', { payload: { annotation: { id: 'old', type: 9, rect, segmentRects: [rect] } } });
    await buildAnnotatedPdf('p1', [row('a1'), legacy], h.deps);
    expect(h.order).toEqual(['open', 'import', 'create', 'download', 'close']);
    expect(h.created).toEqual([[PAGE.ref, expect.objectContaining({ subtype: 'highlight', nm: 'old' })]]);
  });

  it('closes the throwaway even when the download fails', async () => {
    const h = harness({
      download: vi.fn(async () => {
        throw new Error('saving failed');
      }),
    });
    await expect(buildAnnotatedPdf('p1', [row('a1')], h.deps)).rejects.toThrow('saving failed');
    // A leaked document would hold a PDFium handle for the rest of the session.
    expect(h.deps.close).toHaveBeenCalledWith(exportDocumentId('p1'));
  });

  it('closes the throwaway when the document never opens', async () => {
    const h = harness({
      open: vi.fn(async () => {
        throw new Error('404');
      }),
    });
    await expect(buildAnnotatedPdf('p1', [row('a1')], h.deps)).rejects.toThrow('404');
    expect(h.deps.close).toHaveBeenCalledWith(exportDocumentId('p1'));
  });

  it('refuses rather than exporting a copy without the marks', async () => {
    const h = harness({ scope: vi.fn(() => null) });
    await expect(buildAnnotatedPdf('p1', [row('a1')], h.deps)).rejects.toThrow(/could not take annotations/);
    expect(h.deps.download).not.toHaveBeenCalled();
    expect(h.deps.close).toHaveBeenCalled();
  });

  it('reports the export’s own failure, not a failed close', async () => {
    const h = harness({
      download: vi.fn(async () => {
        throw new Error('saving failed');
      }),
      close: vi.fn(async () => {
        throw new Error('already closed');
      }),
    });
    await expect(buildAnnotatedPdf('p1', [row('a1')], h.deps)).rejects.toThrow('saving failed');
  });

  it('skips a row whose payload cannot be rebuilt rather than failing', async () => {
    const h = harness();
    await buildAnnotatedPdf('p1', [row('a1'), row('bad', { payload: null })], h.deps);
    const [[bundle]] = h.imported;
    expect(bundle.items.map((item) => item.data.nm)).toEqual(['a1']);
  });
});
