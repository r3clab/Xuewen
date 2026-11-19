import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

interface Row {
  item: { ref: { objectNumber: number }; title: string; target: unknown };
  depth: number;
  hasChildren: boolean;
  expanded: boolean;
}

vi.mock('@embedpdf/svelte/outline', () => ({
  useOutline: () => outline,
  useOutlineState: () => tree,
}));
vi.mock('@embedpdf/svelte/stage', () => ({ useStage: () => stage }));

import PdfOutline from './PdfOutline.svelte';

const intro = { kind: 'xyz', page: { objectNumber: 3 }, x: 72, y: 96, zoom: null };
const method = { kind: 'xyz', page: { objectNumber: 5 }, x: 72, y: 410, zoom: null };

function row(objectNumber: number, title: string, depth: number, target: unknown, hasChildren = false): Row {
  return { item: { ref: { objectNumber }, title, target }, depth, hasChildren, expanded: true };
}

/// The plugin's tree: every item, and what folding shows of it.
const ITEMS = [
  row(1, '1 Introduction', 0, { kind: 'goto', destination: intro }, true),
  row(2, '1.1 Threat model', 1, { kind: 'goto', destination: method }),
  row(4, 'Project page', 0, { kind: 'uri', uri: 'https://example.com' }),
];
const tree = $state({ status: 'idle', rows: ITEMS, current: null as { objectNumber: number } | null });
const stage = { goToDestination: vi.fn() };

const outline = {
  ensureLoaded: vi.fn(async () => {
    tree.status = 'ready';
  }),
  collapseAll: vi.fn(() => {
    tree.rows = ITEMS.filter((r) => r.depth === 0).map((r) => ({ ...r, expanded: false }));
  }),
  reveal: vi.fn(() => {
    tree.rows = ITEMS;
  }),
  toggle: vi.fn(),
};

beforeEach(() => {
  vi.clearAllMocks();
  tree.status = 'idle';
  tree.rows = ITEMS;
  tree.current = null;
});

describe('PdfOutline', () => {
  it('opens a first read folded to its top level', async () => {
    render(PdfOutline);
    expect(await screen.findByRole('button', { name: '1 Introduction' })).toBeInTheDocument();
    expect(outline.collapseAll).toHaveBeenCalledOnce();
    expect(screen.queryByRole('button', { name: '1.1 Threat model' })).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Expand section' })).toHaveAttribute('aria-expanded', 'false');
  });

  it('reveals the current section and scrolls its row into view', async () => {
    tree.current = { objectNumber: 2 };
    const scrolled = vi.spyOn(Element.prototype, 'scrollIntoView');
    render(PdfOutline);
    const current = await screen.findByRole('button', { name: '1.1 Threat model' });
    expect(outline.reveal).toHaveBeenCalledWith({ objectNumber: 2 });
    expect(current).toHaveAttribute('aria-current', 'location');
    expect(scrolled.mock.contexts).toEqual([current]);
    scrolled.mockRestore();
  });

  it("keeps the reader's folding when the outline was already read", async () => {
    tree.status = 'ready';
    render(PdfOutline);
    expect(await screen.findByRole('button', { name: '1.1 Threat model' })).toBeInTheDocument();
    expect(outline.collapseAll).not.toHaveBeenCalled();
  });

  it('jumps to where an entry points on its page, and offers no jump for a web link', async () => {
    tree.status = 'ready';
    render(PdfOutline);
    await userEvent.click(await screen.findByRole('button', { name: '1.1 Threat model' }));
    expect(stage.goToDestination).toHaveBeenCalledWith(method);
    expect(screen.getByRole('button', { name: 'Project page' })).toBeDisabled();
  });
});
