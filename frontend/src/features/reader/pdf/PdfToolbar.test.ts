import { render, screen } from '@testing-library/svelte';
import { describe, expect, it, vi } from 'vitest';

const stage = vi.hoisted(() => ({ goToPage: vi.fn(), zoomTo: vi.fn() }));
vi.mock('@embedpdf/svelte/stage', async (importOriginal) => ({
  ...(await importOriginal<object>()),
  useStage: () => stage,
  useStageState: () => ({ currentPageIndex: 0, pageCount: 3, zoomLevel: 1, zoomMode: 'fit-page' }),
}));
vi.mock('@embedpdf/svelte/runtime', () => ({ useOptionalCapability: () => ({ current: null }) }));
vi.mock('@embedpdf/svelte/annotation', () => ({ AnnotationToken: 'annotation' }));
vi.mock('@embedpdf/svelte/interaction', () => ({ InteractionToken: 'interaction' }));
vi.mock('@embedpdf/svelte/history', () => ({ useHistoryState: () => ({ canUndo: false, canRedo: false }) }));

import userEvent from '@testing-library/user-event';
import PdfToolbar from './PdfToolbar.svelte';
import { pdfAppearance } from '$shared/ui/theme.svelte';
import { dismissTopLayer } from '$shared/ui/escape';

// Mirrors the PillHide interface (pdf/pillHide.svelte.ts) the toolbar binds to;
// the component invokes pillEnter/pillLeave/focusIn/focusOut on pointer/focus
// events, so all must be present or the events throw.
const pill = {
  visible: true,
  toolbarVisible: true,
  host: () => {},
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

  it('is one Tab stop, with the arrows moving between its controls', async () => {
    render(PdfToolbar, { props: { documentId: 'd1', pill } });
    const sidebar = screen.getByRole('button', { name: 'Toggle sidebar' });
    const tabStops = screen
      .getByRole('toolbar', { name: 'PDF controls' })
      .querySelectorAll('button[tabindex="0"], input[tabindex="0"]');
    expect([...tabStops]).toEqual([sidebar]);
    sidebar.focus();
    const find = screen.getByRole('button', { name: 'Find in document' });
    await userEvent.keyboard('{End}');
    expect(find).toHaveFocus();
    expect(find.tabIndex).toBe(0);
    expect(sidebar.tabIndex).toBe(-1);
    await userEvent.keyboard('{ArrowRight}'); // wraps
    expect(sidebar).toHaveFocus();
    // Previous page is disabled on page 1, so the arrow skips it; inside the
    // page field the arrows move the caret instead.
    await userEvent.keyboard('{ArrowRight}');
    const page = screen.getByRole('textbox', { name: 'Page number' });
    expect(page).toHaveFocus();
    await userEvent.keyboard('{ArrowRight}');
    expect(page).toHaveFocus();
  });

  it('Esc abandons a page-number edit, and the edit holds the pill up', async () => {
    window.addEventListener('keydown', dismissTopLayer);
    const { component } = render(PdfToolbar, { props: { documentId: 'd1', pill } });
    const input = screen.getByRole('textbox', { name: 'Page number' });
    await userEvent.click(input);
    await userEvent.keyboard('3');
    expect(component.held()).toBe(true);
    await userEvent.keyboard('{Escape}');
    expect(input).not.toHaveFocus();
    expect(input).toHaveValue('1');
    expect(component.held()).toBe(false);
    expect(stage.goToPage).not.toHaveBeenCalled();
    window.removeEventListener('keydown', dismissTopLayer);
  });

  it('Enter jumps to the typed page', async () => {
    render(PdfToolbar, { props: { documentId: 'd1', pill } });
    await userEvent.click(screen.getByRole('textbox', { name: 'Page number' }));
    await userEvent.keyboard('3{Enter}');
    expect(stage.goToPage).toHaveBeenCalledWith(2);
  });

  it('the zoom menu takes focus and holds the pill up while open', async () => {
    window.addEventListener('keydown', dismissTopLayer);
    const { component } = render(PdfToolbar, { props: { documentId: 'd1', pill } });
    const trigger = screen.getByRole('button', { name: 'Zoom level' });
    await userEvent.click(trigger);
    expect(component.held()).toBe(true);
    expect(screen.getAllByRole('menuitemradio')[0]).toHaveFocus();
    // The reader opens fitted to the page: that is the checked choice, and
    // no level is, whatever percentage the fit lands on.
    expect(screen.getByRole('menuitemradio', { name: 'Fit page' })).toHaveAttribute('aria-checked', 'true');
    expect(screen.getByRole('menuitemradio', { name: '100%' })).toHaveAttribute('aria-checked', 'false');
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).toBeNull();
    expect(trigger).toHaveFocus();
    expect(component.held()).toBe(false);
    window.removeEventListener('keydown', dismissTopLayer);
  });

  it('page appearance button names current and next state', () => {
    pdfAppearance.mode = 'dim';
    render(PdfToolbar, { props: { documentId: 'd1', pill } });
    const btn = screen.getByRole('button', { name: /page appearance/i });
    expect(btn.title).toMatch(/dimmed — click for inverted/i);
  });
});
