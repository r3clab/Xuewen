import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { PdfDocumentObject } from '@embedpdf/models';
import type { Marker, Reference } from './references';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';
import type { StructuredReference } from '$shared/api/generated/StructuredReference';

vi.mock(import('$features/library/api'), () => ({
  listPapers: vi.fn(),
}));
vi.mock(import('./api'), () => ({
  parseCitations: vi.fn(),
}));
vi.mock(import('./load'), () => ({
  loadCitations: vi.fn(),
}));

import * as api from './api';
import * as libraryApi from '$features/library/api';
import { markLibraryChanged } from '$features/library/state.svelte';
import { runCitationPipeline, type CitationPipelineUpdate } from './pipeline';
import { loadCitations, type EngineLike } from './load';

// loadCitations is mocked, so the engine and document are never touched.
const engine = {} as EngineLike;
const doc = {} as PdfDocumentObject;

const ADAM = 'Adam: A Method for Stochastic Optimization';

function ref(index: number, rawText: string): Reference {
  return { index, destPageIndex: 1, destY: 100 + index * 30, rawText };
}

function marker(refIndex: number): Marker {
  return { pageIndex: 0, x: 10, y: 10, width: 12, height: 12, refIndex };
}

function paper(id: string, title: string): PaperSummary {
  return {
    id, title, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', name: null, starred: false, tags: [], projects: [],
  };
}

function structured(over: Partial<StructuredReference> = {}): StructuredReference {
  return {
    authors: [], title: null, venue: null, year: null, doi: null, arxiv_id: null,
    url: null, ...over,
  };
}

function run(hooks: Partial<Parameters<typeof runCitationPipeline>[3]> = {}) {
  const updates: CitationPipelineUpdate[] = [];
  const done = runCitationPipeline(engine, doc, 'd1', {
    isCancelled: () => false,
    onUpdate: (u) => updates.push(u),
    ...hooks,
  });
  return { updates, done };
}

beforeEach(() => {
  vi.clearAllMocks();
  markLibraryChanged();
  vi.mocked(libraryApi.listPapers).mockResolvedValue([paper('p1', ADAM)]);
  vi.mocked(api.parseCitations).mockResolvedValue(null);
});

describe('runCitationPipeline', () => {
  it('publishes extraction, then matches, then the structured upgrade', async () => {
    const raw = ref(0, `D. Kingma, J. Ba. ${ADAM}. ICLR 2015.`);
    vi.mocked(loadCitations).mockResolvedValue({ references: [raw], markers: [marker(0)] });
    vi.mocked(api.parseCitations).mockResolvedValue([structured({ title: ADAM, year: 2015 })]);
    const { updates, done } = run();
    await done;
    expect(updates).toHaveLength(3);
    // Phase 1: raw extraction only — hovers work before any matching.
    expect(updates[0]).toEqual({ citations: { references: [raw], markers: [marker(0)] } });
    // Phase 2: library matches, citations untouched.
    expect(updates[1].citations).toBeUndefined();
    expect(updates[1].matches?.get(0)?.id).toBe('p1');
    // Phase 3: structured refs + final matches together.
    expect(updates[2].citations?.references[0].structured?.title).toBe(ADAM);
    expect(updates[2].matches?.get(0)?.id).toBe('p1');
    expect(api.parseCitations).toHaveBeenCalledWith('d1', [raw.rawText]);
  });

  it('keeps raw text when the structured parse is unavailable', async () => {
    const raw = ref(0, `D. Kingma, J. Ba. ${ADAM}. ICLR 2015.`);
    vi.mocked(loadCitations).mockResolvedValue({ references: [raw], markers: [] });
    const { updates, done } = run();
    await done;
    expect(updates[2].citations?.references).toEqual([raw]);
  });

  it('skips the parse entirely when extraction found no references', async () => {
    vi.mocked(loadCitations).mockResolvedValue({ references: [], markers: [] });
    const { done } = run();
    await done;
    expect(api.parseCitations).not.toHaveBeenCalled();
  });

  it('resolves author-year candidates against the merged references', async () => {
    const raw = ref(0, 'Kingma, D. and Ba, J. (2015). Adam. ICLR.');
    vi.mocked(loadCitations).mockResolvedValue({
      references: [raw],
      markers: [],
      pendingAuthorYear: [
        { pageIndex: 0, x: 10, y: 10, width: 40, height: 12, citeText: 'Kingma and Ba, 2015' },
      ],
    });
    vi.mocked(api.parseCitations).mockResolvedValue([
      structured({ authors: ['Diederik Kingma', 'Jimmy Ba'], year: 2015 }),
    ]);
    const { updates, done } = run();
    await done;
    expect(updates[2].citations?.markers).toEqual([
      { pageIndex: 0, x: 10, y: 10, width: 40, height: 12, refIndex: 0 },
    ]);
  });

  it('publishes nothing more once cancelled between awaits', async () => {
    const raw = ref(0, `D. Kingma, J. Ba. ${ADAM}. ICLR 2015.`);
    vi.mocked(loadCitations).mockResolvedValue({ references: [raw], markers: [] });
    let cancelled = false;
    const updates: CitationPipelineUpdate[] = [];
    await runCitationPipeline(engine, doc, 'd1', {
      isCancelled: () => cancelled,
      onUpdate: (u) => {
        updates.push(u);
        cancelled = true; // cancel right after the first publish
      },
    });
    expect(updates).toHaveLength(1);
  });

  it('publishes nothing at all when cancelled before extraction lands', async () => {
    vi.mocked(loadCitations).mockResolvedValue({ references: [], markers: [] });
    const { updates, done } = run({ isCancelled: () => true });
    await done;
    expect(updates).toEqual([]);
  });

  it('swallows an extraction failure — the reader still works', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.mocked(loadCitations).mockRejectedValue(new Error('worker died'));
    const { updates, done } = run();
    await expect(done).resolves.toBeUndefined();
    expect(updates).toEqual([]);
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });
});
