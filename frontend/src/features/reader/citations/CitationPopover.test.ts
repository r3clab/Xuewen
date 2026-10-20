import { render } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, afterEach, vi } from 'vitest';

vi.mock(import('$features/importing/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/importing/api')>();
  return {
    ...mod,
    importUrl: vi.fn(async (): Promise<import('$shared/api/generated/ImportResult').ImportResult> => ({
      outcome: 'ingested', id: 'p-new', title: 'Fuzzgen', status: 'resolved',
    })),
  };
});
vi.mock(import('$features/library/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/api')>();
  return {
    ...mod,
    listPapers: vi.fn(async () => []),
    getStats: vi.fn(async () => ({ total: 0, resolved: 0, needs_review: 0 })),
  };
});

import CitationPopover from './CitationPopover.svelte';
import { citationHover } from './state.svelte';
import { importState } from '$features/importing/queue.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

afterEach(() => {
  citationHover.current = null;
  importState.items = [];
  importState.cancelled = false;
  importState.open = false;
});

const paper: PaperSummary = {
  id: 'paper-1', title: 'Adam', authors: [], venue: null, year: null, doi: null,
  arxiv_id: null, dblp_key: null, cite_key: 'k2015adam', url: null, source: null,
  status: 'resolved', added_at: '2020-01-01', name: null, starred: false, tags: [], projects: [],
};

function show(ref: Partial<import('./references').Reference>, matchedPaper: PaperSummary | null) {
  citationHover.current = {
    reference: { index: 0, destPageIndex: 1, destY: 100, rawText: 'raw ref text', ...ref },
    matchedPaper,
    screenX: 10,
    screenY: 10,
  };
}

describe('CitationPopover', () => {
  it('always shows the raw reference text', async () => {
    show({ rawText: 'Kingma & Ba. Adam. ICLR 2015.' }, null);
    const { findByText, queryByRole } = render(CitationPopover);
    expect(await findByText(/Adam\. ICLR 2015\./)).toBeInTheDocument();
    expect(queryByRole('button', { name: /open in library/i })).not.toBeInTheDocument();
    expect(queryByRole('link')).not.toBeInTheDocument();
  });

  it('shows Open in library only when matched', async () => {
    show({}, paper);
    const { findByRole, queryByRole } = render(CitationPopover);
    expect(await findByRole('button', { name: /open in library/i })).toBeInTheDocument();
    expect(queryByRole('link')).not.toBeInTheDocument();
  });

  it('shows an external link when the entry has a url', async () => {
    show({ externalUrl: 'https://doi.org/10.1/x' }, null);
    const { findByRole, queryByRole } = render(CitationPopover);
    const link = await findByRole('link', { name: /doi\.org/i });
    expect(link).toHaveAttribute('href', 'https://doi.org/10.1/x');
    expect(queryByRole('button', { name: /open in library/i })).not.toBeInTheDocument();
  });

  it('renders a structured card when the reference is parsed', async () => {
    show(
      {
        structured: {
          authors: ['D. Kingma', 'J. Ba'], title: 'Adam: A Method for Stochastic Optimization',
          venue: 'ICLR', year: 2015,
          doi: null, arxiv_id: '1412.6980', url: null,
        },
      },
      null,
    );
    const { findByText, queryByText, findByRole } = render(CitationPopover);
    expect(await findByText('Adam: A Method for Stochastic Optimization')).toBeInTheDocument();
    expect(await findByText(/D\. Kingma, J\. Ba/)).toBeInTheDocument();
    expect(await findByText(/ICLR.*2015/)).toBeInTheDocument(); // venue (via abbreviateVenue) + year
    expect(await findByRole('link', { name: /arXiv/i })).toBeInTheDocument();
    expect(queryByText('raw ref text')).not.toBeInTheDocument(); // structured replaces raw
  });

  it('falls back to raw text when structured is null', async () => {
    show({ structured: null }, null);
    const { findByText } = render(CitationPopover);
    expect(await findByText('raw ref text')).toBeInTheDocument();
  });

  it('offers Import for an unmatched reference with a DOI', async () => {
    show(
      {
        structured: {
          authors: [], title: 'Fuzzgen', venue: 'USENIX Security', year: 2020,
          doi: '10.1234/fuzzgen', arxiv_id: null, url: null,
        },
      },
      null,
    );
    const { findByRole } = render(CitationPopover);
    expect(await findByRole('button', { name: /^import$/i })).toBeInTheDocument();
  });

  it('does not offer Import when the reference is already in the library', async () => {
    show(
      {
        structured: {
          authors: [], title: 'Adam', venue: null, year: null,
          doi: '10.1234/adam', arxiv_id: null, url: null,
        },
      },
      paper,
    );
    const { queryByRole, findByRole } = render(CitationPopover);
    await findByRole('button', { name: /open in library/i });
    expect(queryByRole('button', { name: /^import$/i })).not.toBeInTheDocument();
  });

  it('clicking Import enqueues the identifier into the freshly opened modal', async () => {
    show(
      {
        structured: {
          authors: [], title: 'Fuzzgen', venue: 'USENIX Security', year: 2020,
          doi: '10.1234/fuzzgen', arxiv_id: null, url: null,
        },
      },
      null,
    );
    const { findByRole } = render(CitationPopover);
    await userEvent.click(await findByRole('button', { name: /^import$/i }));
    // openImport() resets importState.items — the enqueue must come AFTER it,
    // or the just-queued item is wiped and the modal opens empty.
    expect(importState.open).toBe(true);
    expect(importState.items).toHaveLength(1);
    expect(importState.items[0].name).toBe('10.1234/fuzzgen');
    expect(citationHover.current).toBeNull();
  });
});
