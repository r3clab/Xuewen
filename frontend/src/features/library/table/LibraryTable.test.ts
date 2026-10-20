import { fireEvent, render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi, type Mock } from 'vitest';

vi.mock(import('$features/library/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/api')>();
  return {
    ...mod,
    listPapers: vi.fn(async () => []),
    setStar: vi.fn(async () => {}),
    deletePaper: vi.fn(async () => {}),
    restorePaper: vi.fn(async () => {}),
    getStats: vi.fn(async () => ({ total: 0, resolved: 0, needs_review: 0 })),
  };
});
vi.mock(import('$features/library/organization/api'), async (importOriginal) => {
  const mod = await importOriginal<typeof import('$features/library/organization/api')>();
  return {
    ...mod,
    listProjects: vi.fn(async () => []),
    listTags: vi.fn(async () => []),
    addTag: vi.fn(async (_id: string, name: string) => ({ id: `t-${name}`, name })),
    addPaperToProject: vi.fn(async () => {}),
  };
});

import * as api from '$features/library/api';
import * as organizationApi from '$features/library/organization/api';
import LibraryTable from './LibraryTable.svelte';
import { columnWidths, resetColumnWidths } from './columnWidths.svelte';
import { ICON_COLUMN_PX, PINNED_COLUMNS, PINNED_KEYS, TAGS_MIN_PX } from './columns';
import { library } from '$features/library/state.svelte';
import { projects } from '$features/library/organization/state.svelte';
import { filters, searchMeta } from '$features/library/search/state.svelte';
import { selection, viewer } from '$features/reader/tabs/state.svelte';
import { toasts } from '$shared/ui/toasts.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

function paper(id: string, title: string, extra: Partial<PaperSummary> = {}): PaperSummary {
  return {
    id, name: null, title, authors: ['Ada Lovelace', 'Alan Turing', 'Grace Hopper'], venue: 'NDSS',
    year: 2026, doi: null, arxiv_id: null, dblp_key: null, cite_key: null, url: null,
    source: null, status: 'resolved', added_at: '2026-07-01T00:00:00Z', starred: false,
    tags: [], projects: [], ...extra,
  };
}

// This jsdom build has no PointerEvent; MouseEvent carries the same fields
// the resize action reads (clientX/button), and pointer capture is try/caught.
function pointerEvent(type: string, clientX: number): Event {
  const Ctor: typeof MouseEvent =
    typeof PointerEvent === 'undefined' ? MouseEvent : (PointerEvent as unknown as typeof MouseEvent);
  return new Ctor(type, { clientX, button: 0, bubbles: true, cancelable: true });
}

beforeEach(() => {
  vi.clearAllMocks();
  library.papers = [paper('p1', 'First Paper'), paper('p2', 'Second Paper', { starred: true })];
  projects.items = [{ id: 'pr1', name: 'RTOS Fuzzing', paper_count: 0, created_at: '' }];
  viewer.tabs = [];
  viewer.activeId = null;
  selection.id = null;
  toasts.items.length = 0;
  resetColumnWidths(); // module singleton — a prior test's widths would leak
  localStorage.clear();
  Object.assign(filters, {
    q: '', status: 'all', sort: 'year_desc', project: 'all', tag: undefined, starred: undefined,
  });
  searchMeta.byId = {};
});

describe('LibraryTable', () => {
  it('renders one row per paper and opens a paper from its title', async () => {
    render(LibraryTable);
    expect(screen.getAllByRole('row')).toHaveLength(3); // header + 2 papers
    expect(screen.getAllByText('Ada Lovelace')).toHaveLength(2); // first-author column
    expect(screen.getAllByText('Grace Hopper')).toHaveLength(2); // last-author column
    await userEvent.click(screen.getByRole('button', { name: 'First Paper' }));
    expect(viewer.activeId).toBe('p1');
  });

  it('renders the Name column: value when set, em-dash ghost when not', () => {
    library.papers = [paper('p1', 'First Paper', { name: 'RVSpec' }), paper('p2', 'Second Paper')];
    render(LibraryTable);
    expect(screen.getByText('RVSpec')).toBeInTheDocument();
    expect(screen.getByText('RVSpec').closest('[data-col="name"]')).not.toBeNull();
    // Mono semibold at full ink: the name is a handle to scan for, not the
    // muted metadata the neighbouring columns are.
    const cell = screen.getByText('RVSpec').closest('td')!;
    expect(cell.className).toContain('font-semibold');
    expect(cell.className).toContain('text-ink');
    // p2 has no name -> its Name cell shows the ghost dash (venue/year cells
    // are populated by the fixture, so at least one dash comes from Name).
    expect(screen.getAllByText('—').length).toBeGreaterThanOrEqual(1);
  });

  it('the Name header sets the name sort (single direction) and reloads', async () => {
    render(LibraryTable);
    await userEvent.click(screen.getByRole('button', { name: /^name$/i }));
    expect(filters.sort).toBe('name');
    expect(api.listPapers as Mock).toHaveBeenCalled();
    // Clicking again keeps it ascending — no toggle like Year.
    await userEvent.click(screen.getByRole('button', { name: /^name$/i }));
    expect(filters.sort).toBe('name');
  });

  it('the Year header toggles sort direction and reloads', async () => {
    render(LibraryTable);
    await userEvent.click(screen.getByRole('button', { name: /^year/i }));
    expect(filters.sort).toBe('year_asc');
    expect(api.listPapers as Mock).toHaveBeenCalled();
    await userEvent.click(screen.getByRole('button', { name: /^year/i }));
    expect(filters.sort).toBe('year_desc');
  });

  it('selecting rows shows the bulk bar; select-all and clear work', async () => {
    render(LibraryTable);
    await userEvent.click(screen.getByRole('checkbox', { name: /select first paper/i }));
    expect(screen.getByText('1 selected')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('checkbox', { name: /select all/i }));
    expect(screen.getByText('2 selected')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: /clear selection/i }));
    expect(screen.queryByText(/selected/)).not.toBeInTheDocument();
  });

  it('bulk star stars only the unstarred selected papers', async () => {
    render(LibraryTable);
    await userEvent.click(screen.getByRole('checkbox', { name: /select all/i }));
    await userEvent.click(screen.getByRole('button', { name: /^star$/i }));
    await vi.waitFor(() => {
      expect((api.setStar as Mock).mock.calls).toEqual([['p1', true]]); // p2 already starred
    });
  });

  it('bulk tag adds the tag to every selected paper', async () => {
    render(LibraryTable);
    await userEvent.click(screen.getByRole('checkbox', { name: /select all/i }));
    await userEvent.type(screen.getByPlaceholderText(/add tag/i), 'nlp/eval');
    await userEvent.click(screen.getByRole('button', { name: /apply tag/i }));
    await vi.waitFor(() => {
      expect((organizationApi.addTag as Mock).mock.calls.map((c) => c[0])).toEqual(['p1', 'p2']);
      expect((organizationApi.addTag as Mock).mock.calls.every((c) => c[1] === 'nlp/eval')).toBe(true);
    });
  });

  it('bulk add-to-project adds every selected paper', async () => {
    render(LibraryTable);
    await userEvent.click(screen.getByRole('checkbox', { name: /select all/i }));
    await userEvent.selectOptions(screen.getByRole('combobox', { name: /add to project/i }), 'pr1');
    await vi.waitFor(() => {
      expect((organizationApi.addPaperToProject as Mock).mock.calls.map((c) => c[0])).toEqual(['p1', 'p2']);
    });
  });

  it('bulk delete confirms, deletes all, and shows one combined Undo toast', async () => {
    render(LibraryTable);
    await userEvent.click(screen.getByRole('checkbox', { name: /select all/i }));
    await userEvent.click(screen.getByRole('button', { name: /^delete$/i }));
    expect(api.deletePaper as Mock).not.toHaveBeenCalled(); // confirm first
    await userEvent.click(screen.getByRole('button', { name: 'Delete 2' }));
    await vi.waitFor(() => {
      expect((api.deletePaper as Mock).mock.calls.map((c) => c[0])).toEqual(['p1', 'p2']);
    });
    const undoToasts = toasts.items.filter((t) => t.action);
    expect(undoToasts).toHaveLength(1);
    expect(undoToasts[0].message).toMatch(/2 papers deleted/);
  });

  it('a failed bulk tag surfaces an error toast instead of vanishing silently', async () => {
    (organizationApi.addTag as Mock).mockRejectedValueOnce(new Error('tag exploded'));
    render(LibraryTable);
    await userEvent.click(screen.getByRole('checkbox', { name: /select all/i }));
    await userEvent.type(screen.getByPlaceholderText(/add tag/i), 'nlp');
    await userEvent.click(screen.getByRole('button', { name: /apply tag/i }));
    await vi.waitFor(() => {
      expect(toasts.items.some((t) => t.kind === 'error' && /tag exploded/.test(t.message))).toBe(
        true,
      );
    });
  });

  it('a partial bulk add-to-project failure reports the failed count', async () => {
    (organizationApi.addPaperToProject as Mock)
      .mockImplementationOnce(async () => {})
      .mockRejectedValueOnce(new Error('boom'));
    render(LibraryTable);
    await userEvent.click(screen.getByRole('checkbox', { name: /select all/i }));
    await userEvent.selectOptions(screen.getByRole('combobox', { name: /add to project/i }), 'pr1');
    await vi.waitFor(() => {
      expect(
        toasts.items.some((t) => t.kind === 'error' && /1 of 2 papers.*boom/.test(t.message)),
      ).toBe(true);
    });
  });

  it('highlights the j/k selection cursor row', async () => {
    selection.id = 'p2';
    render(LibraryTable);
    const row = screen.getByRole('button', { name: 'Second Paper' }).closest('tr')!;
    expect(row.dataset.cursor).toBe('true');
  });

  it('hides sort arrows and disables sort buttons while a search is active', () => {
    filters.q = 'fuzzing';
    render(LibraryTable);
    const year = screen.getByRole('button', { name: 'Year' });
    expect(year).toBeDisabled();
    expect(year.title).toMatch(/relevance/i);
    // No aria-sort claim while relevance-ranked.
    for (const th of screen.getAllByRole('columnheader')) {
      expect(th).not.toHaveAttribute('aria-sort');
    }
  });

  it('keeps sort headers live for a qualifier-only filter, which still lists in sort order', () => {
    filters.q = 'tag:nlp';
    render(LibraryTable);
    expect(screen.getByRole('button', { name: 'Year' })).not.toBeDisabled();
  });

  it('shows em-dash placeholders for missing metadata', () => {
    library.papers = [paper('p3', 'Mystery Paper', { authors: [], venue: null, year: null })];
    render(LibraryTable);
    // first author + last author + venue + year each show a placeholder dash
    expect(screen.getAllByText('—').length).toBeGreaterThanOrEqual(4);
  });

  it('flags needs-review papers with a pill, and only those', () => {
    library.papers = [paper('p1', 'First Paper'), paper('p3', 'Mystery Paper', { status: 'needs_review' })];
    render(LibraryTable);
    expect(screen.getAllByText('Needs review')).toHaveLength(1);
    expect(screen.getByText('Needs review').closest('tr')).toHaveTextContent('Mystery Paper');
  });

  it('shows the search snippet under the title while searching', () => {
    filters.q = 'fuzz';
    searchMeta.byId = {
      p1: { engine: 'keyword', field: 'body', page: 7, snippet: 'a <mark>fuzz</mark>er' },
    };
    render(LibraryTable);
    expect(screen.getByText('fuzz').tagName).toBe('MARK');
    expect(screen.getByText(/body/).closest('tr')).toHaveTextContent('First Paper');
    expect(screen.getByText(/p\.7/)).toBeInTheDocument();
    expect(screen.getAllByRole('row')).toHaveLength(3); // the snippet adds no row
  });

  it('hides a stale snippet once the query no longer searches', () => {
    filters.q = 'tag:nlp';
    searchMeta.byId = {
      p1: { engine: 'keyword', field: 'body', page: 7, snippet: 'a <mark>fuzz</mark>er' },
    };
    render(LibraryTable);
    expect(screen.queryByText('fuzz')).not.toBeInTheDocument();
  });

  it('a single-author paper shows that author in both author columns', () => {
    library.papers = [paper('p4', 'Solo Work', { authors: ['Ada Lovelace'] })];
    render(LibraryTable);
    expect(screen.getAllByText('Ada Lovelace')).toHaveLength(2);
  });

  it('shows the abbreviated venue in the cell and the raw venue in the tooltip', () => {
    const raw = '2019 IEEE Symposium on Security and Privacy (SP)';
    library.papers = [paper('p5', 'Secure Paper', { venue: raw })];
    render(LibraryTable);
    const cell = screen.getByText('S&P');
    expect(cell.closest('[title]')?.getAttribute('title')).toBe(raw);
  });

  it('keeps every row one line high however many tags a paper carries', () => {
    library.papers = [
      paper('p1', 'First Paper', {
        projects: [{ id: 'pr1', name: 'RTOS Fuzzing' }],
        tags: [
          { id: 't1', name: 'security/fuzzing' },
          { id: 't2', name: 'os/rtos' },
          { id: 't3', name: 'ml/llm' },
          { id: 't4', name: 'benchmarks' },
        ],
      }),
    ];
    render(LibraryTable);
    const strip = screen.getByText('security/fuzzing').parentElement!;
    expect(strip.className).toMatch(/overflow-hidden/);
    expect(strip.parentElement!.className).not.toMatch(/flex-wrap|mt-1\.5/);
  });

  it('reserves a floor for the Tags column rather than letting it collapse', () => {
    render(LibraryTable);
    const pinned = PINNED_KEYS.reduce((s, k) => s + columnWidths[k], 0);
    expect(screen.getByRole('table').style.minWidth).toBe(
      `${2 * ICON_COLUMN_PX + pinned + TAGS_MIN_PX}px`,
    );
  });

  it('renders one resize separator per pinned column', () => {
    render(LibraryTable);
    expect(screen.getAllByRole('separator')).toHaveLength(7);
  });

  it('dragging a handle commits and persists the width', async () => {
    render(LibraryTable);
    const handle = screen.getByRole('separator', { name: 'Resize Title column' });
    await fireEvent(handle, pointerEvent('pointerdown', 100));
    await fireEvent(handle, pointerEvent('pointermove', 150));
    await fireEvent(handle, pointerEvent('pointerup', 150));
    expect(columnWidths.title).toBe(PINNED_COLUMNS.title.defaultWidth + 50);
    const saved = JSON.parse(localStorage.getItem('xuewen-library-columns') ?? '{}');
    expect(saved.title).toBe(PINNED_COLUMNS.title.defaultWidth + 50);
    // A real browser fires a post-drag click that consumes the one-shot
    // swallower; jsdom doesn't, so drain it or it leaks into the next test.
    window.dispatchEvent(new MouseEvent('click'));
  });

  it('double-clicking a handle auto-fits that column to its content', async () => {
    render(LibraryTable);
    await fireEvent.dblClick(screen.getByRole('separator', { name: 'Resize Title column' }));
    // jsdom has no canvas, so the estimate measurer runs: the short fixture
    // titles land the column at its minimum — down from the default.
    expect(columnWidths.title).toBe(PINNED_COLUMNS.title.minWidth);
    expect(localStorage.getItem('xuewen-library-columns')).not.toBeNull();
  });

  it('right-clicking the header offers reset-to-defaults', async () => {
    render(LibraryTable);
    columnWidths.title = 400;
    await fireEvent.contextMenu(screen.getAllByRole('row')[0]);
    await userEvent.click(screen.getByRole('menuitem', { name: /reset to default widths/i }));
    expect(columnWidths.title).toBe(PINNED_COLUMNS.title.defaultWidth);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('auto-fit all from the header menu commits and persists the pinned columns', async () => {
    render(LibraryTable);
    await fireEvent.contextMenu(screen.getAllByRole('row')[0]);
    await userEvent.click(screen.getByRole('menuitem', { name: /auto-fit all columns/i }));
    expect(columnWidths.title).toBeLessThan(PINNED_COLUMNS.title.defaultWidth);
    const saved = JSON.parse(localStorage.getItem('xuewen-library-columns') ?? '{}');
    expect(saved.title).toBe(columnWidths.title);
  });
});
