import { beforeEach, describe, expect, it, vi } from 'vitest';
import { applyIdentify } from '$app/actions';
import { classifyIdentifyInput, dropsIdentifier, pseudoDoiHint } from './input';
import { closeIdentify, openIdentify, runIdentifySearch } from './actions';
import { identifyState } from './state.svelte';
import { cachedDetail } from '$features/library/state.svelte';

const CAND = {
  title: 'AntiFuzz: Impeding Fuzzing Audits of Binary Executables',
  abstract: null,
  authors: ['Emre Güler'],
  venue: 'USENIX Security Symposium',
  year: 2019,
  doi: null,
  arxiv_id: null,
  dblp_key: 'conf/uss/GulerAAH19',
  url: null,
  source: 'dblp',
};

function json(o: unknown, status = 200): Response {
  return new Response(JSON.stringify(o), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

describe('identify', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    openIdentify('paper-1');
  });

  it('classifies input as doi, arxiv, or title', () => {
    expect(classifyIdentifyInput('10.1145/3292500.3330701')).toEqual({
      kind: 'doi',
      value: '10.1145/3292500.3330701',
    });
    expect(classifyIdentifyInput('https://doi.org/10.1145/329.001')).toEqual({
      kind: 'doi',
      value: '10.1145/329.001',
    });
    // Trailing copy-paste punctuation is stripped from a DOI.
    expect(classifyIdentifyInput('10.1145/3292500.3330701.')).toEqual({
      kind: 'doi',
      value: '10.1145/3292500.3330701',
    });
    expect(classifyIdentifyInput('(10.1145/3292500.3330701),')).toEqual({
      kind: 'doi',
      value: '10.1145/3292500.3330701',
    });
    expect(classifyIdentifyInput('1706.03762v5')).toEqual({ kind: 'arxiv', value: '1706.03762v5' });
    // arXiv URLs are recognized symmetrically with doi.org URLs.
    expect(classifyIdentifyInput('https://arxiv.org/abs/1706.03762v5')).toEqual({
      kind: 'arxiv',
      value: '1706.03762v5',
    });
    expect(classifyIdentifyInput('https://arxiv.org/pdf/1706.03762')).toEqual({
      kind: 'arxiv',
      value: '1706.03762',
    });
    expect(classifyIdentifyInput('AntiFuzz: Impeding')).toEqual({
      kind: 'title',
      value: 'AntiFuzz: Impeding',
    });
  });

  it('searches titles and applies a picked candidate', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string | URL, init?: RequestInit) => {
        const u = String(url);
        if (u.startsWith('/api/identify/search')) return json([CAND]);
        if (u.endsWith('/identify') && init?.method === 'POST') {
          const body = JSON.parse(String(init.body));
          expect(body.candidate.dblp_key).toBe('conf/uss/GulerAAH19');
          return json({ id: 'paper-1', title: CAND.title, status: 'resolved' });
        }
        if (u.startsWith('/api/papers')) return json([]);
        return json({ total: 0, resolved: 0, needs_review: 0 });
      }),
    );

    identifyState.input = 'AntiFuzz: Impeding';
    await runIdentifySearch();
    expect(identifyState.candidates.length).toBe(1);

    identifyState.selected = identifyState.candidates[0];
    await applyIdentify();
    expect(identifyState.open).toBe(false); // closed on success
    expect(identifyState.error).toBeNull();
    // Open panels see the applied record through the reactive detail cache.
    expect(cachedDetail('paper-1')?.title).toBe(CAND.title);
  });

  it('stages a direct DOI without fetching and applies it as a doi body', async () => {
    const fetchSpy = vi.fn(async (url: string | URL, init?: RequestInit) => {
      const u = String(url);
      if (u.endsWith('/identify') && init?.method === 'POST') {
        expect(JSON.parse(String(init.body))).toEqual({ doi: '10.1145/3292500.3330701' });
        return json({ id: 'paper-1', title: 'T', status: 'resolved' });
      }
      if (u.startsWith('/api/papers')) return json([]);
      return json({ total: 0, resolved: 0, needs_review: 0 });
    });
    vi.stubGlobal('fetch', fetchSpy);

    identifyState.input = '10.1145/3292500.3330701';
    await runIdentifySearch();
    expect(identifyState.direct).toEqual({ doi: '10.1145/3292500.3330701' });
    expect(fetchSpy).not.toHaveBeenCalled(); // staging is purely local

    await applyIdentify();
    expect(identifyState.open).toBe(false);
    expect(identifyState.error).toBeNull();
  });

  it('clears a previous selection when a new search runs', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string | URL) => {
        const u = String(url);
        if (u.startsWith('/api/identify/search')) return json([CAND]);
        return json([]);
      }),
    );

    identifyState.input = 'AntiFuzz: Impeding';
    await runIdentifySearch();
    identifyState.selected = identifyState.candidates[0];

    identifyState.input = '10.1234/x';
    await runIdentifySearch();
    expect(identifyState.selected).toBeNull();
    expect(identifyState.candidates.length).toBe(0);
    expect(identifyState.direct).toEqual({ doi: '10.1234/x' });
  });

  it('surfaces conflict errors inline', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string | URL, init?: RequestInit) => {
        const u = String(url);
        if (u.endsWith('/identify') && init?.method === 'POST') {
          return json({ error: 'same work as abc123', id: 'abc123' }, 409);
        }
        return json([]);
      }),
    );

    identifyState.selected = CAND;
    await applyIdentify();
    expect(identifyState.open).toBe(true); // stays open
    expect(identifyState.error).toContain('same work as abc123');
  });

  it('drops stale search results from a superseded identify session', async () => {
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string | URL) => {
        const u = String(url);
        if (u.startsWith('/api/identify/search')) {
          await gate;
          return json([CAND]);
        }
        return json([]);
      }),
    );

    openIdentify('a');
    identifyState.input = 'AntiFuzz: Impeding';
    const inflight = runIdentifySearch();

    // The modal is closed and reopened for another paper mid-flight.
    closeIdentify();
    openIdentify('b');

    release();
    await inflight;
    expect(identifyState.candidates.length).toBe(0); // stale result dropped
    expect(identifyState.paperId).toBe('b');
    expect(identifyState.busy).toBe(false);
  });

  it('closeIdentify resets state', () => {
    identifyState.input = 'x';
    identifyState.error = 'y';
    closeIdentify();
    expect(identifyState.open).toBe(false);
    expect(identifyState.input).toBe('');
    expect(identifyState.error).toBeNull();
  });

  it('dropsIdentifier flags a selected candidate missing an identifier the paper currently has', () => {
    openIdentify('paper-1', { doi: '10.1/x', arxiv_id: null });
    identifyState.selected = { ...CAND, doi: null };
    expect(dropsIdentifier(identifyState)).toBe(true);
  });

  it('dropsIdentifier is false when nothing is selected', () => {
    openIdentify('paper-1', { doi: '10.1/x', arxiv_id: null });
    expect(dropsIdentifier(identifyState)).toBe(false);
  });

  it('dropsIdentifier is false when the selected candidate keeps the identifier', () => {
    openIdentify('paper-1', { doi: '10.1/x', arxiv_id: null });
    identifyState.selected = { ...CAND, doi: '10.1/x' };
    expect(dropsIdentifier(identifyState)).toBe(false);
  });

  it('warns when staging an ACM 10.5555 pseudo-DOI', () => {
    // ACM DL uses 10.5555/… for papers with NO registered DOI (e.g. USENIX);
    // it never resolves via Crossref/doi.org.
    expect(pseudoDoiHint({ doi: '10.5555/3361338.3361472' })).toMatch(/title search/);
    expect(pseudoDoiHint({ doi: '10.1145/3292500.3330701' })).toBeNull();
    expect(pseudoDoiHint({ arxiv_id: '1706.03762' })).toBeNull();
    expect(pseudoDoiHint(null)).toBeNull();
  });
});
