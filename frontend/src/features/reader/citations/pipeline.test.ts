import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Marker, Reference } from './references';
import type { CitedReference } from '$shared/api/generated/CitedReference';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';
import type { StructuredReference } from '$shared/api/generated/StructuredReference';

vi.mock(import('./api'), () => ({
  matchCitations: vi.fn(),
  parseCitations: vi.fn(),
}));
vi.mock(import('./load'), () => ({
  loadCitations: vi.fn(),
}));

import * as api from './api';
import { runCitationPipeline, type CitationPipelineUpdate } from './pipeline';
import { loadCitations, type CitationSource } from './load';

// loadCitations is mocked, so the source is never read.
const source: CitationSource = {
  pages: [],
  links: () => [],
  readRuns: async () => [],
  pageIndexOf: () => -1,
};

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

function run(hooks: Partial<Parameters<typeof runCitationPipeline>[2]> = {}) {
  const updates: CitationPipelineUpdate[] = [];
  const done = runCitationPipeline(source, 'd1', {
    isCancelled: () => false,
    onUpdate: (u) => updates.push(u),
    ...hooks,
  });
  return { updates, done };
}

// Stands in for the server's matcher: a reference cites the library's one
// paper when its text or parsed title contains that paper's title.
function matchAdam(refs: CitedReference[]): (PaperSummary | null)[] {
  return refs.map((r) => (r.text.includes(ADAM) || r.title === ADAM ? paper('p1', ADAM) : null));
}

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(api.matchCitations).mockImplementation(async (refs) => matchAdam(refs));
  vi.mocked(api.parseCitations).mockResolvedValue([]);
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

  it('keeps raw text and the raw matches when the structured parse fails', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const raw = ref(0, `D. Kingma, J. Ba. ${ADAM}. ICLR 2015.`);
    vi.mocked(loadCitations).mockResolvedValue({ references: [raw], markers: [] });
    vi.mocked(api.parseCitations).mockRejectedValue(new Error('parsing citations failed: 500'));
    const { updates, done } = run();
    await done;
    expect(updates[2].citations?.references).toEqual([raw]);
    expect(updates[2].matches?.get(0)?.id).toBe('p1');
    expect(api.matchCitations).toHaveBeenCalledTimes(1);
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });

  it('matches again on the titles the structured parse found', async () => {
    const raw = ref(0, 'D. Kingma, J. Ba. Adam: a method for stochastic optimization (2015)');
    vi.mocked(loadCitations).mockResolvedValue({ references: [raw], markers: [] });
    vi.mocked(api.parseCitations).mockResolvedValue([structured({ title: ADAM })]);
    const { updates, done } = run();
    await done;
    expect(updates[1].matches?.size).toBe(0);
    expect(api.matchCitations).toHaveBeenLastCalledWith([{ text: raw.rawText, title: ADAM }]);
    expect(updates[2].matches?.get(0)?.id).toBe('p1');
  });

  it('asks the server nothing when extraction found no references', async () => {
    vi.mocked(loadCitations).mockResolvedValue({ references: [], markers: [] });
    const { updates, done } = run();
    await done;
    expect(updates).toHaveLength(1);
    expect(api.matchCitations).not.toHaveBeenCalled();
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
    await runCitationPipeline(source, 'd1', {
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
