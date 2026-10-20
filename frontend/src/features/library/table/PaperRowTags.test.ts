import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it } from 'vitest';
import PaperRowTags from './PaperRowTags.svelte';
import { filters } from '$features/library/search/state.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

const paper: PaperSummary = {
  id: 'p1',
  title: 't',
  authors: [],
  venue: null,
  year: null,
  doi: null,
  arxiv_id: null,
  dblp_key: null,
  cite_key: null,
  url: null,
  source: null,
  status: 'resolved',
  added_at: '',
  name: null,
  starred: false,
  projects: [],
  tags: [
    { id: 't1', name: 'security/fuzzing' },
    { id: 't2', name: 'os/rtos' },
    { id: 't3', name: 'ml/llm' },
    { id: 't4', name: 'benchmarks' },
    { id: 't5', name: 'robotics' },
  ],
};

beforeEach(() => {
  filters.tag = undefined;
});

describe('PaperRowTags', () => {
  it('caps tag chips at 3 with a +N overflow control', () => {
    render(PaperRowTags, { props: { paper } });
    expect(screen.getByText('security/fuzzing')).toBeInTheDocument();
    expect(screen.getByText('os/rtos')).toBeInTheDocument();
    expect(screen.getByText('ml/llm')).toBeInTheDocument();
    expect(screen.queryByText('benchmarks')).not.toBeInTheDocument();
    expect(screen.queryByText('robotics')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: '+2' })).toBeInTheDocument();
  });

  it('reveals all 5 tags when the +2 control is clicked', async () => {
    render(PaperRowTags, { props: { paper } });
    await userEvent.click(screen.getByRole('button', { name: '+2' }));
    expect(screen.getByText('security/fuzzing')).toBeInTheDocument();
    expect(screen.getByText('os/rtos')).toBeInTheDocument();
    expect(screen.getByText('ml/llm')).toBeInTheDocument();
    expect(screen.getByText('benchmarks')).toBeInTheDocument();
    expect(screen.getByText('robotics')).toBeInTheDocument();
    // The +N is gone, but a "Less" control replaces it so the tags can fold back.
    expect(screen.queryByRole('button', { name: /^\+\d/ })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Less' })).toBeInTheDocument();
  });

  it('folds the tags back when "Less" is clicked', async () => {
    render(PaperRowTags, { props: { paper } });
    await userEvent.click(screen.getByRole('button', { name: '+2' }));
    await userEvent.click(screen.getByRole('button', { name: 'Less' }));
    // Back to the capped view: overflow tags hidden and the +2 control restored.
    expect(screen.queryByText('benchmarks')).not.toBeInTheDocument();
    expect(screen.queryByText('robotics')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: '+2' })).toBeInTheDocument();
  });

  it('renders project badges that never count toward the tag cap', () => {
    const withProject = { ...paper, projects: [{ id: 'pr1', name: 'RTOS Fuzzing' }] };
    render(PaperRowTags, { props: { paper: withProject } });
    expect(screen.getByText('RTOS Fuzzing')).toBeInTheDocument();
    // Still only 3 tag chips are shown + a +2 (the badge is not part of the cap).
    expect(screen.getByRole('button', { name: '+2' })).toBeInTheDocument();
    expect(screen.queryByText('benchmarks')).not.toBeInTheDocument();
  });

  // The chip strip is the element the chips are children of; expanded, it is
  // `display: contents`, so it is structurally the same either way and only
  // the classes differ.
  function strip() {
    return screen.getByText('security/fuzzing').parentElement!;
  }

  it('stays on one clipped line while collapsed', () => {
    render(PaperRowTags, { props: { paper } });
    // One line: no wrap, which made a tag-heavy table row taller than its
    // neighbours.
    expect(strip().parentElement!.className).not.toMatch(/flex-wrap/);
    expect(strip().className).toMatch(/overflow-hidden/);
    // Chips clip rather than squeeze, and the tooltip names the ones the cut
    // edge hides.
    expect(screen.getByText('security/fuzzing').className).toMatch(/shrink-0/);
    expect(strip()).toHaveAttribute(
      'title',
      'security/fuzzing, os/rtos, ml/llm, benchmarks, robotics',
    );
  });

  it('wraps once expanded, so +N still reveals everything', async () => {
    render(PaperRowTags, { props: { paper } });
    await userEvent.click(screen.getByRole('button', { name: '+2' }));
    // Growing taller is fine when it was asked for — and "Less" undoes it.
    expect(strip().className).toBe('contents');
    expect(strip().parentElement!.className).toMatch(/flex-wrap/);
    expect(screen.getByText('robotics')).toBeInTheDocument();
  });

  it('keeps a tag matching the active filter visible beyond the cap', () => {
    filters.tag = 'robotics';
    render(PaperRowTags, { props: { paper } });
    expect(screen.getByText('robotics')).toBeInTheDocument();
    // benchmarks (index 3, no match) stays hidden; robotics (index 4) is
    // pulled forward for the filter hit, so only 1 remains truly hidden.
    expect(screen.queryByText('benchmarks')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: '+1' })).toBeInTheDocument();
  });
});
