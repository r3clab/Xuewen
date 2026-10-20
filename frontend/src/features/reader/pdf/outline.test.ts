import { describe, expect, it } from 'vitest';
import { PdfActionType, PdfZoomMode } from '@embedpdf/models';
import type { PdfBookmarkObject } from '@embedpdf/models';
import { currentOutlinePath, toOutline, type OutlineNode } from './outline';

const dest = (pageIndex: number) =>
  ({ pageIndex, zoom: { mode: PdfZoomMode.Unknown }, view: [] }) as never;

describe('toOutline', () => {
  it('flattens titles, depths, and destination pages', () => {
    const bookmarks: PdfBookmarkObject[] = [
      {
        title: 'Introduction',
        target: { type: 'destination', destination: dest(0) },
        children: [{ title: 'Motivation', target: { type: 'destination', destination: dest(1) } }],
      },
      { title: 'Evaluation', target: { type: 'destination', destination: dest(7) } },
    ];
    const nodes = toOutline(bookmarks);
    expect(nodes).toHaveLength(2);
    expect(nodes[0]).toMatchObject({ title: 'Introduction', pageIndex: 0, depth: 0 });
    expect(nodes[0].children[0]).toMatchObject({ title: 'Motivation', pageIndex: 1, depth: 1 });
    expect(nodes[1]).toMatchObject({ title: 'Evaluation', pageIndex: 7, depth: 0 });
  });

  it('resolves Goto actions and leaves other targets unclickable', () => {
    const bookmarks: PdfBookmarkObject[] = [
      {
        title: 'Via action',
        target: { type: 'action', action: { type: PdfActionType.Goto, destination: dest(4) } },
      },
      {
        title: 'External link',
        target: { type: 'action', action: { type: PdfActionType.URI, uri: 'https://example.com' } },
      },
      { title: 'No target' },
    ];
    const nodes = toOutline(bookmarks);
    expect(nodes[0].pageIndex).toBe(4);
    expect(nodes[1].pageIndex).toBe(null);
    expect(nodes[2].pageIndex).toBe(null);
    expect(nodes[2].children).toEqual([]);
  });
});

describe('currentOutlinePath', () => {
  const node = (title: string, pageIndex: number | null, children: OutlineNode[] = []): OutlineNode => ({
    title,
    pageIndex,
    depth: 0, // depth is irrelevant to path computation
    children,
  });

  it('returns null for an empty outline or before the first section', () => {
    expect(currentOutlinePath([], 5)).toBe(null);
    expect(currentOutlinePath([node('Intro', 2)], 0)).toBe(null);
  });

  it('matches a section from its start page', () => {
    const nodes = [node('Intro', 0), node('Method', 3)];
    expect(currentOutlinePath(nodes, 0)).toBe('0');
    expect(currentOutlinePath(nodes, 2)).toBe('0'); // inside Intro
    expect(currentOutlinePath(nodes, 3)).toBe('1');
    expect(currentOutlinePath(nodes, 9)).toBe('1'); // past the last start
  });

  it('prefers the deepest subsection already begun', () => {
    const nodes = [node('Intro', 0, [node('Motivation', 1), node('Contributions', 2)]), node('Method', 4)];
    expect(currentOutlinePath(nodes, 1)).toBe('0.0');
    expect(currentOutlinePath(nodes, 3)).toBe('0.1'); // between last subsection and Method
    expect(currentOutlinePath(nodes, 4)).toBe('1'); // later top-level beats earlier subsection
  });

  it('skips unpaged nodes but still walks their children', () => {
    const nodes = [node('Part I', null, [node('Background', 1)]), node('Part II', null, [node('Design', 5)])];
    expect(currentOutlinePath(nodes, 2)).toBe('0.0');
    expect(currentOutlinePath(nodes, 6)).toBe('1.0');
    expect(currentOutlinePath([node('Cover', null)], 3)).toBe(null); // only unpaged nodes
  });
});
