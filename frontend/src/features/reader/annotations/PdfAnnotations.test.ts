import { render, screen, waitFor } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { PdfAnnotationSubtype } from '@embedpdf/models';

const scroll = { scrollToPage: vi.fn() };
const annotation = {
  selectAnnotation: vi.fn(),
  deleteAnnotation: vi.fn(),
  getAnnotationById: vi.fn(() => undefined as unknown),
};

/// Enough of the export plumbing for the button to run end to end: a throwaway
/// document that opens, reports its own marks loaded, and saves four bytes.
const exportScope = {
  importAnnotations: vi.fn(),
  commit: vi.fn(() => ({ toPromise: async () => true })),
  onAnnotationEvent: vi.fn((h: (e: { type: string }) => void) => {
    loadedHandler = h;
    return () => {};
  }),
};
let loadedHandler: ((e: { type: string }) => void) | null = null;
const openTask = {
  toPromise: async () => ({
    documentId: 'export:p1',
    task: {
      toPromise: async () => {
        loadedHandler?.({ type: 'loaded' });
        return { id: 'export:p1' };
      },
    },
  }),
};
const documents = {
  openDocumentUrl: vi.fn(() => openTask),
  closeDocument: vi.fn(),
};
const saveAsCopy = vi.fn(() => ({ toPromise: async () => new Uint8Array([1, 2, 3, 4]).buffer }));

vi.mock('@embedpdf/plugin-scroll/svelte', () => ({
  useScroll: () => ({ state: { currentPage: 1, totalPages: 9 }, provides: scroll }),
}));
vi.mock('@embedpdf/plugin-annotation/svelte', () => ({
  useAnnotation: () => ({ provides: annotation, state: {} }),
  useAnnotationCapability: () => ({
    provides: { forDocument: () => exportScope },
    isLoading: false,
  }),
}));
vi.mock('@embedpdf/plugin-document-manager/svelte', () => ({
  useDocumentManagerCapability: () => ({ provides: documents, isLoading: false }),
}));
vi.mock('@embedpdf/core/svelte', () => ({
  useRegistry: () => ({ registry: { getEngine: () => ({ saveAsCopy }) } }),
}));

const deleteAnnotation = vi.fn(async (_paperId: string, _id: string) => {});
vi.mock(import('./api'), () => ({
  deleteAnnotation: (paperId: string, id: string) => deleteAnnotation(paperId, id),
}));
vi.mock(import('$features/reader/pdf/api'), () => ({
  pdfUrl: (id: string) => `/papers/${id}.pdf`,
}));

// The real filename rule, but jsdom has no URL.createObjectURL to hand a Blob to.
const downloadBlob = vi.fn();
vi.mock(import('./download'), async (importOriginal) => ({
  ...(await importOriginal<typeof import('./download')>()),
  downloadBlob: (blob: Blob, filename: string) => downloadBlob(blob, filename),
}));

import PdfAnnotations from './PdfAnnotations.svelte';
import { annotations } from './store.svelte';
import { colorHex } from './palette';
import { viewer } from '$features/reader/tabs/state.svelte';
import { toasts } from '$shared/ui/toasts.svelte';
import type { Annotation } from '$shared/api/generated/Annotation';

function row(over: Partial<Annotation> & { id: string }): Annotation {
  return {
    paper_id: 'p1',
    kind: 'highlight',
    page_index: 0,
    color: 'amber',
    quoted_text: null,
    note: null,
    payload: {
      annotation: {
        id: over.id,
        type: PdfAnnotationSubtype.HIGHLIGHT,
        pageIndex: over.page_index ?? 0,
        rect: { origin: { x: 0, y: 0 }, size: { width: 10, height: 10 } },
      },
    },
    created_at: '2026-08-14T00:00:00Z',
    updated_at: '2026-08-14T00:00:00Z',
    ...over,
  };
}

function seed(...rows: Annotation[]): void {
  annotations.byPaper['p1'] = Object.fromEntries(rows.map((r) => [r.id, r]));
  annotations.loaded['p1'] = true;
}

beforeEach(() => {
  vi.clearAllMocks();
  annotations.byPaper = {};
  annotations.loaded = {};
  annotations.error = {};
  annotation.getAnnotationById.mockReturnValue(undefined);
  loadedHandler = null;
  viewer.tabs = [{ id: 'p1', title: 'Attention Is All You Need', name: null }];
  toasts.items = [];
});

const props = { documentId: 'p1' };

describe('the annotations panel', () => {
  it('says how to make one when the paper has none', () => {
    seed();
    render(PdfAnnotations, { props });
    expect(screen.getByText(/no annotations yet/i)).toBeInTheDocument();
  });

  it('reports a failed load instead of pretending the paper is unannotated', () => {
    annotations.error['p1'] = 'offline';
    render(PdfAnnotations, { props });
    expect(screen.getByText(/could not be loaded/i)).toBeInTheDocument();
    expect(screen.queryByText(/no annotations yet/i)).toBeNull();
  });

  it('lists marks in reading order with their quoted text and note', () => {
    seed(
      row({ id: 'b', page_index: 4, kind: 'underline', quoted_text: 'second' }),
      row({ id: 'a', page_index: 1, quoted_text: 'first', note: 'why it matters' }),
    );
    render(PdfAnnotations, { props });
    const rows = screen.getAllByRole('listitem');
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent('first');
    expect(rows[0]).toHaveTextContent('why it matters');
    expect(rows[0]).toHaveTextContent('p.2'); // page_index is 0-based, the label is not
    expect(rows[1]).toHaveTextContent('Underline');
    expect(rows[1]).toHaveTextContent('p.5');
  });

  it('carries the mark’s color, so the panel matches the page', () => {
    seed(row({ id: 'a', color: 'violet', quoted_text: 'q' }));
    render(PdfAnnotations, { props });
    // The swatch is decorative; the color still has to reach a screen reader.
    expect(screen.getByText('Violet')).toBeInTheDocument();
    expect(screen.getByText('q')).toHaveStyle({ borderColor: colorHex('violet') });
  });

  it('shows a placeholder for a sticky note with nothing written in it yet', () => {
    seed(row({ id: 'a', kind: 'text_comment' }));
    render(PdfAnnotations, { props });
    expect(screen.getByText('(no text)')).toBeInTheDocument();
  });

  it('jumps to the page and selects the mark when a row is clicked', async () => {
    seed(row({ id: 'a', page_index: 6, quoted_text: 'there' }));
    render(PdfAnnotations, { props });
    await userEvent.click(screen.getByText('there'));
    expect(scroll.scrollToPage).toHaveBeenCalledWith({ pageNumber: 7 });
    expect(annotation.selectAnnotation).toHaveBeenCalledWith(6, 'a');
  });

  it('deletes through the plugin when the mark is drawn, letting sync remove the row', async () => {
    seed(row({ id: 'a', page_index: 2, quoted_text: 'drawn' }));
    annotation.getAnnotationById.mockReturnValue({ id: 'a' });
    render(PdfAnnotations, { props });
    await userEvent.click(screen.getByRole('button', { name: /^delete highlight/i }));
    expect(annotation.deleteAnnotation).toHaveBeenCalledWith(2, 'a');
    // Deleting twice — once here and once again through the store — would
    // 404 the second call and log an error the reader can do nothing about.
    expect(deleteAnnotation).not.toHaveBeenCalled();
  });

  it('deletes through the store when the mark never reached the document', async () => {
    seed(row({ id: 'a', quoted_text: 'orphan' }));
    render(PdfAnnotations, { props });
    await userEvent.click(screen.getByRole('button', { name: /^delete highlight/i }));
    expect(annotation.deleteAnnotation).not.toHaveBeenCalled();
    expect(deleteAnnotation).toHaveBeenCalledWith('p1', 'a');
    expect(screen.queryByText('orphan')).toBeNull();
  });
});

describe('exporting an annotated copy', () => {
  it('is not offered until there is something to export', () => {
    seed();
    render(PdfAnnotations, { props });
    expect(screen.queryByRole('button', { name: /export/i })).toBeNull();
  });

  it('saves a copy named after the paper, leaving the open document alone', async () => {
    seed(row({ id: 'a', quoted_text: 'q' }));
    render(PdfAnnotations, { props });
    await userEvent.click(screen.getByRole('button', { name: /export annotated pdf/i }));
    await waitFor(() => expect(downloadBlob).toHaveBeenCalled());
    expect(downloadBlob.mock.calls[0][1]).toBe('Attention Is All You Need (annotated).pdf');
    // A throwaway document, never the tab's own: the library file has to stay
    // byte-identical, so the marks are written into a second copy.
    expect(documents.openDocumentUrl).toHaveBeenCalledWith(
      expect.objectContaining({ documentId: 'export:p1', autoActivate: false }),
    );
    expect(exportScope.importAnnotations).toHaveBeenCalledWith([
      expect.objectContaining({ annotation: expect.objectContaining({ id: 'a' }) }),
    ]);
    expect(documents.closeDocument).toHaveBeenCalledWith('export:p1');
  });

  it('says so and recovers when the engine cannot save', async () => {
    saveAsCopy.mockReturnValueOnce({
      toPromise: async () => {
        throw { code: 3, message: 'saving failed' };
      },
    });
    seed(row({ id: 'a', quoted_text: 'q' }));
    render(PdfAnnotations, { props });
    const button = screen.getByRole('button', { name: /export annotated pdf/i });
    await userEvent.click(button);
    await waitFor(() => expect(toasts.items.map((t) => t.message)).toContain('saving failed'));
    expect(downloadBlob).not.toHaveBeenCalled();
    // Back to a button the reader can press again, not stuck on "Exporting…".
    await waitFor(() => expect(button).toBeEnabled());
  });
});
