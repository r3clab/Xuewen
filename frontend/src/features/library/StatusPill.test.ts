import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import StatusPill from './StatusPill.svelte';

describe('StatusPill', () => {
  it('renders nothing for resolved papers (the normal state)', () => {
    const { container } = render(StatusPill, { props: { status: 'resolved' } });
    expect(container.textContent?.trim()).toBe('');
  });

  it('flags unresolved papers as needing review', () => {
    render(StatusPill, { props: { status: 'needs_review' } });
    expect(screen.getByText('Needs review')).toBeInTheDocument();
  });

  it('appends a caller class to the pill', () => {
    render(StatusPill, { props: { status: 'needs_review', class: 'ml-1.5' } });
    expect(screen.getByText('Needs review')).toHaveClass('ml-1.5');
  });
});
