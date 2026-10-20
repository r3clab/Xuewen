import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import PreviewFallbackCard from './PreviewFallbackCard.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

function paper(over: Partial<PaperSummary> = {}): PaperSummary {
  return {
    id: 'p1', title: 'Attention Is All You Need', authors: ['A', 'B'], venue: 'NeurIPS',
    year: 2017, doi: null, arxiv_id: null, dblp_key: null, cite_key: null, url: null,
    source: null, status: 'resolved', added_at: '', name: null, starred: false,
    tags: [], projects: [], ...over,
  };
}

describe('PreviewFallbackCard', () => {
  it('stands in for the pages with what the row already carries', () => {
    render(PreviewFallbackCard, { paper: paper() });
    expect(screen.getByText('Attention Is All You Need')).toBeInTheDocument();
    expect(screen.getByText('A, B')).toBeInTheDocument();
    expect(screen.getByText('NeurIPS · 2017')).toBeInTheDocument();
    expect(screen.getByText(/could not be rendered/)).toBeInTheDocument();
  });

  it('elides a long author list', () => {
    render(PreviewFallbackCard, { paper: paper({ authors: ['A', 'B', 'C', 'D'] }) });
    expect(screen.getByText('A, B, C et al.')).toBeInTheDocument();
  });

  it('drops the metadata lines it has nothing for', () => {
    render(PreviewFallbackCard, { paper: paper({ authors: [], venue: null, year: null, title: null }) });
    expect(screen.getByText('(untitled)')).toBeInTheDocument();
    expect(screen.queryByText('·')).not.toBeInTheDocument();
  });
});
