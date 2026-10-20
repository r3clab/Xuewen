import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

vi.mock('@embedpdf/plugin-zoom/svelte', () => ({
  useZoom: () => ({ state: { currentZoomLevel: 1 }, provides: null }),
}));
vi.mock('@embedpdf/plugin-scroll/svelte', () => ({
  useScroll: () => ({ state: { currentPage: 1, totalPages: 3 }, provides: null }),
}));

import userEvent from '@testing-library/user-event';
import PdfToolbar from './PdfToolbar.svelte';
import { pdfAppearance } from '$shared/ui/theme.svelte';

// Mirrors the PillHide interface (pdf/pillHide.svelte.ts) the toolbar binds to;
// the component invokes pillEnter/pillLeave/focusIn/focusOut on pointer/focus
// events, so all must be present or the events throw.
const pill = {
  visible: true,
  toolbarVisible: true,
  setHost: () => {},
  setExtraHold: () => {},
  onWindowMove: () => {},
  onScroll: () => {},
  onScrollJump: () => {},
  pillEnter: () => {},
  pillLeave: () => {},
  focusIn: () => {},
  focusOut: () => {},
} as never;

describe('PdfToolbar', () => {
  it('cycles the dark-mode page appearance from its toolbar button', async () => {
    pdfAppearance.mode = 'normal';
    render(PdfToolbar, { props: { documentId: 'd1', pill } });
    const btn = screen.getByRole('button', { name: /page appearance/i });
    await userEvent.click(btn);
    expect(pdfAppearance.mode).toBe('dim');
    await userEvent.click(btn);
    expect(pdfAppearance.mode).toBe('invert');
  });

  it('gives icon-only nav and zoom buttons hover tooltips', () => {
    render(PdfToolbar, { props: { documentId: 'd1', pill } });
    for (const name of ['Previous page', 'Next page', 'Zoom out', 'Zoom in']) {
      expect(screen.getByRole('button', { name })).toHaveAttribute('title');
    }
  });

  it('page appearance button names current and next state', () => {
    pdfAppearance.mode = 'dim';
    render(PdfToolbar, { props: { documentId: 'd1', pill } });
    const btn = screen.getByRole('button', { name: /page appearance/i });
    expect(btn.title).toMatch(/dimmed — click for inverted/i);
  });
});
