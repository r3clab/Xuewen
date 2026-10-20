import { render, screen } from '@testing-library/svelte';
import { describe, expect, it } from 'vitest';
import Spinner from './Spinner.svelte';

describe('Spinner', () => {
  it('renders an accessible loading indicator with the given label', () => {
    render(Spinner, { props: { label: 'Loading reader…' } });
    expect(screen.getByRole('status')).toBeInTheDocument();
    expect(screen.getByText('Loading reader…')).toBeInTheDocument();
  });

  it('defaults the label to Loading…', () => {
    render(Spinner);
    expect(screen.getByRole('status')).toBeInTheDocument();
    expect(screen.getByText('Loading…')).toBeInTheDocument();
  });
});
