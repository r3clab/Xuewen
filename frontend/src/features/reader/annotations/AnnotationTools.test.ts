import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';

type Patcher = (selected: { subtype: string }) => object;

const { marks, hub } = vi.hoisted(() => ({
  marks: {
    tools: { updateDefaults: vi.fn() },
    selection: {
      getSelected: vi.fn((): readonly unknown[] => []),
      update: vi.fn(async (_patch: Patcher) => ({})),
    },
  },
  hub: { activateTool: vi.fn(), activateDefaultTool: vi.fn() },
}));

vi.mock('@embedpdf/svelte/annotation', () => ({ AnnotationToken: 'annotation' }));
vi.mock('@embedpdf/svelte/interaction', () => ({ InteractionToken: 'interaction' }));
vi.mock('@embedpdf/svelte/runtime', () => ({
  useOptionalCapability: (token: string) => ({ current: token === 'annotation' ? marks : hub }),
}));

import AnnotationTools from './AnnotationTools.svelte';
import { annotationTools } from './state.svelte';
import { colorHex } from './palette';
import { dismissTopLayer } from '$shared/ui/escape';

const props = {};

beforeEach(() => {
  vi.clearAllMocks();
  localStorage.clear();
  annotationTools.active = null;
  annotationTools.color = 'amber';
  marks.selection.getSelected.mockReturnValue([]);
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
    expect(screen.queryByRole('menuitemradio', { name: /ink|square|circle|free text/i })).toBeNull();
  });

  it('opens from the bound state the toolbar holds', () => {
    render(AnnotationTools, { props: { menuOpen: true } });
    expect(screen.getByRole('menu', { name: 'Annotation tools' })).toBeInTheDocument();
  });

  it('takes focus into the menu, and arrows move through its items', async () => {
    render(AnnotationTools, { props });
    await openMenu();
    expect(screen.getByRole('menuitemradio', { name: 'Highlight' })).toHaveFocus();
    await userEvent.keyboard('{ArrowDown}');
    expect(screen.getByRole('menuitemradio', { name: 'Underline' })).toHaveFocus();
  });

  it('closes on Escape and hands focus back to its trigger', async () => {
    window.addEventListener('keydown', dismissTopLayer);
    render(AnnotationTools, { props });
    await openMenu();
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).toBeNull();
    expect(screen.getByRole('button', { name: /annotation tools/i })).toHaveFocus();
    window.removeEventListener('keydown', dismissTopLayer);
  });
});

describe('arming a tool', () => {
  it('tells the plugin which tool is armed', async () => {
    render(AnnotationTools, { props });
    await openMenu();
    await userEvent.click(screen.getByRole('menuitemradio', { name: 'Highlight' }));
    expect(annotationTools.active).toBe('highlight');
    expect(hub.activateTool).toHaveBeenLastCalledWith('highlight');
  });

  it('uses the plugin tool id, not the wire kind, for a sticky note', async () => {
    render(AnnotationTools, { props });
    await openMenu();
    await userEvent.click(screen.getByRole('menuitemradio', { name: 'Note' }));
    // The wire spelling is `text_comment`; the plugin's tool is `note`.
    expect(hub.activateTool).toHaveBeenLastCalledWith('note');
  });

  it('disarms when the armed tool is clicked again', async () => {
    render(AnnotationTools, { props });
    await openMenu();
    const highlight = screen.getByRole('menuitemradio', { name: 'Highlight' });
    await userEvent.click(highlight);
    hub.activateDefaultTool.mockClear();
    await userEvent.click(highlight);
    expect(annotationTools.active).toBeNull();
    expect(hub.activateDefaultTool).toHaveBeenCalledOnce();
  });

  it('inherits the armed tool when a second document mounts', () => {
    annotationTools.active = 'squiggly';
    render(AnnotationTools, { props });
    // Driven by an effect, not a click: a newly opened tab must not look
    // disarmed while app state says otherwise.
    expect(hub.activateTool).toHaveBeenCalledWith('squiggly');
  });
});

describe('picking a color', () => {
  it('remembers the color across a reload', async () => {
    render(AnnotationTools, { props });
    await openMenu();
    await userEvent.click(screen.getByRole('menuitemradio', { name: 'Blue' }));
    expect(localStorage.getItem('xuewen-annotation-color')).toBe('blue');
  });

  it('arms every tool of the document with the palette color', async () => {
    render(AnnotationTools, { props });
    expect(marks.tools.updateDefaults).toHaveBeenCalledWith('highlight', { color: colorHex('amber') });
    await openMenu();
    await userEvent.click(screen.getByRole('menuitemradio', { name: 'Violet' }));
    for (const tool of ['highlight', 'underline', 'strikeout', 'squiggly', 'note']) {
      expect(marks.tools.updateDefaults).toHaveBeenCalledWith(tool, { color: colorHex('violet') });
    }
  });

  it('recolors what is selected, so a just-drawn mark can be corrected', async () => {
    marks.selection.getSelected.mockReturnValue([{}]);
    render(AnnotationTools, { props });
    await openMenu();
    await userEvent.click(screen.getByRole('menuitemradio', { name: 'Green' }));
    expect(marks.selection.update).toHaveBeenCalledOnce();
    const patch = marks.selection.update.mock.calls[0][0];
    expect(patch({ subtype: 'highlight' })).toEqual({ color: colorHex('green') });
    expect(patch({ subtype: 'text' })).toEqual({ color: colorHex('green') });
    // A foreign mark the reader happens to have selected is left alone.
    expect(patch({ subtype: 'ink' })).toEqual({});
  });

  it('leaves marks alone when nothing is selected', async () => {
    render(AnnotationTools, { props });
    await openMenu();
    await userEvent.click(screen.getByRole('menuitemradio', { name: 'Rose' }));
    expect(marks.selection.update).not.toHaveBeenCalled();
  });
});
