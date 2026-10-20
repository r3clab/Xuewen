import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { PdfAnnotationSubtype } from '@embedpdf/models';

const scope = {
  setActiveTool: vi.fn(),
  getSelectedAnnotations: vi.fn(() => [] as unknown[]),
  updateAnnotation: vi.fn(),
};

vi.mock('@embedpdf/plugin-annotation/svelte', () => ({
  useAnnotation: () => ({ provides: scope, state: {} }),
}));

import AnnotationTools from './AnnotationTools.svelte';
import { annotationTools } from './state.svelte';
import { colorHex } from './palette';

const props = { documentId: 'd1', onHoldChange: () => {} };

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  annotationTools.active = null;
  annotationTools.color = 'amber';
  scope.getSelectedAnnotations.mockReturnValue([]);
});

async function openMenu(): Promise<void> {
  await userEvent.click(screen.getByRole('button', { name: /annotation tools/i }));
}

describe('the tool menu', () => {
  it('stays closed until asked, so the pill is not crowded', () => {
    render(AnnotationTools, { props });
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('offers exactly the five tools and the five palette colors', async () => {
    render(AnnotationTools, { props });
    await openMenu();
    for (const name of ['Highlight', 'Underline', 'Strikeout', 'Squiggly', 'Note']) {
      expect(screen.getByRole('menuitemradio', { name })).toBeInTheDocument();
    }
    for (const name of ['Amber', 'Rose', 'Green', 'Blue', 'Violet']) {
      expect(screen.getByRole('menuitemradio', { name })).toBeInTheDocument();
    }
    // No ink, shapes, or free text in v1.
    expect(
      screen.queryByRole('menuitemradio', { name: /ink|square|circle|free text/i }),
    ).toBeNull();
  });

  it('holds the auto-hiding pill open while the menu is up', async () => {
    const onHoldChange = vi.fn();
    render(AnnotationTools, { props: { ...props, onHoldChange } });
    expect(onHoldChange).toHaveBeenLastCalledWith(false);
    await openMenu();
    expect(onHoldChange).toHaveBeenLastCalledWith(true);
  });

  it('closes on Escape without letting the global cascade see it', async () => {
    const onKey = vi.fn();
    document.addEventListener('keydown', onKey);
    render(AnnotationTools, { props });
    await openMenu();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).toBeNull();
    // The reader's global Escape would exit zen or close the palette.
    expect(onKey).not.toHaveBeenCalled();
    document.removeEventListener('keydown', onKey);
  });
});

describe('arming a tool', () => {
  it('tells the plugin which tool is armed', async () => {
    render(AnnotationTools, { props });
    await openMenu();
    await userEvent.click(screen.getByRole('menuitemradio', { name: 'Highlight' }));
    expect(annotationTools.active).toBe('highlight');
    expect(scope.setActiveTool).toHaveBeenCalledWith('highlight');
  });

  it('uses the plugin tool id, not the wire kind, for a sticky note', async () => {
    render(AnnotationTools, { props });
    await openMenu();
    await userEvent.click(screen.getByRole('menuitemradio', { name: 'Note' }));
    // The wire spelling is `text_comment`; the plugin's tool is `textComment`.
    expect(scope.setActiveTool).toHaveBeenCalledWith('textComment');
  });

  it('disarms when the armed tool is clicked again', async () => {
    render(AnnotationTools, { props });
    await openMenu();
    const highlight = screen.getByRole('menuitemradio', { name: 'Highlight' });
    await userEvent.click(highlight);
    await userEvent.click(highlight);
    expect(annotationTools.active).toBeNull();
    expect(scope.setActiveTool).toHaveBeenLastCalledWith(null);
  });

  it('inherits the armed tool when a second document mounts', async () => {
    annotationTools.active = 'squiggly';
    render(AnnotationTools, { props: { ...props, documentId: 'd2' } });
    // Driven by an effect, not a click: a newly opened tab must not look
    // disarmed while app state says otherwise.
    expect(scope.setActiveTool).toHaveBeenCalledWith('squiggly');
  });
});

describe('picking a color', () => {
  // The push into the plugin's global tool defaults happens in PdfDeck (the
  // defaults are registry-wide, this component is per-tab) — see
  // applyToolDefaults in annotations/adapter.test.ts.
  it('remembers the color across a reload', async () => {
    render(AnnotationTools, { props });
    await openMenu();
    await userEvent.click(screen.getByRole('menuitemradio', { name: 'Blue' }));
    expect(localStorage.getItem('xuewen-annotation-color')).toBe('blue');
  });

  it('recolors what is selected, so a just-drawn mark can be corrected', async () => {
    scope.getSelectedAnnotations.mockReturnValue([
      { object: { id: 'a1', pageIndex: 2, type: PdfAnnotationSubtype.HIGHLIGHT } },
      { object: { id: 'a2', pageIndex: 5, type: PdfAnnotationSubtype.TEXT } },
    ]);
    render(AnnotationTools, { props });
    await openMenu();
    await userEvent.click(screen.getByRole('menuitemradio', { name: 'Green' }));
    const hex = colorHex('green');
    expect(scope.updateAnnotation).toHaveBeenCalledWith(2, 'a1', {
      color: hex,
      strokeColor: hex,
    });
    expect(scope.updateAnnotation).toHaveBeenCalledWith(5, 'a2', { strokeColor: hex });
  });

  it('leaves marks alone when nothing is selected', async () => {
    render(AnnotationTools, { props });
    await openMenu();
    await userEvent.click(screen.getByRole('menuitemradio', { name: 'Rose' }));
    expect(scope.updateAnnotation).not.toHaveBeenCalled();
  });
});
