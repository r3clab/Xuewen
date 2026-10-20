import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import PaperMeta from './PaperMeta.svelte';
import type { PaperDetail } from '$shared/api/generated/PaperDetail';

function paper(overrides: Partial<PaperDetail> = {}): PaperDetail {
  return {
    id: 'p1',
    title: 'A Paper',
    authors: ['A. Author'],
    venue: null,
    year: 2024,
    doi: null,
    arxiv_id: null,
    dblp_key: null,
    cite_key: null,
    url: null,
    source: null,
    status: 'resolved',
    added_at: '2024-01-01',
    name: null,
    starred: false,
    tags: [],
    projects: [],
    abstract: null,
    ...overrides,
  };
}

describe('PaperMeta URL link', () => {
  it('renders an http(s) url as a link', () => {
    render(PaperMeta, { props: { d: paper({ url: 'https://example.com/paper' }) } });
    const link = screen.getByRole('link', { name: 'URL' });
    expect(link).toHaveAttribute('href', 'https://example.com/paper');
  });

  it.each([
    'javascript:alert(1)',
    'data:text/html,<script>alert(1)</script>',
    'vbscript:msgbox(1)',
    'not a url',
  ])('drops the non-web url scheme %j', (url) => {
    render(PaperMeta, { props: { d: paper({ url }) } });
    expect(screen.queryByRole('link', { name: 'URL' })).toBeNull();
  });

  it('still renders hardcoded https DOI/arXiv links alongside a dropped url', () => {
    render(PaperMeta, {
      props: { d: paper({ doi: '10.1/x', url: 'javascript:alert(1)' }) },
    });
    expect(screen.getByRole('link', { name: 'DOI' })).toHaveAttribute(
      'href',
      'https://doi.org/10.1/x',
    );
    expect(screen.queryByRole('link', { name: 'URL' })).toBeNull();
  });
});
