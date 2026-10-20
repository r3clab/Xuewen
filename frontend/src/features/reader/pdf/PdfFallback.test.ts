import { render, screen } from '@testing-library/svelte';
import { describe, it, expect } from 'vitest';
import PdfFallback from './PdfFallback.svelte';

describe('PdfFallback', () => {
  it('links to the raw PDF for open and download', () => {
    render(PdfFallback, { props: { id: 'p1' } });
    const open = screen.getByRole('link', { name: /Open in new tab/ });
    const dl = screen.getByRole('link', { name: /Download/ });
    expect(open).toHaveAttribute('href', '/papers/p1/pdf');
    expect(open).toHaveAttribute('target', '_blank');
    expect(dl).toHaveAttribute('href', '/papers/p1/pdf');
    expect(dl).toHaveAttribute('download');
  });
});
