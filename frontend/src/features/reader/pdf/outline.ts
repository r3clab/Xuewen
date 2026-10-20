import { PdfActionType } from '@embedpdf/models';
import type { PdfBookmarkObject } from '@embedpdf/models';

/// One outline row, render-ready. `pageIndex` is 0-based; null when the
/// bookmark has no resolvable page target (rendered non-clickable).
export interface OutlineNode {
  title: string;
  pageIndex: number | null;
  depth: number;
  children: OutlineNode[];
}

function targetPage(b: PdfBookmarkObject): number | null {
  const t = b.target;
  if (!t) return null;
  if (t.type === 'destination') return t.destination.pageIndex;
  if (t.type === 'action' && t.action.type === PdfActionType.Goto) {
    return t.action.destination.pageIndex;
  }
  return null; // URI / remote-goto / launch actions aren't in-document jumps
}

/// Convert EmbedPDF's bookmark tree into OutlineNodes.
export function toOutline(bookmarks: PdfBookmarkObject[], depth = 0): OutlineNode[] {
  return bookmarks.map((b) => ({
    title: b.title,
    pageIndex: targetPage(b),
    depth,
    children: toOutline(b.children ?? [], depth + 1),
  }));
}

/// Index-path ("2.1") of the outline entry the reader is currently inside:
/// the LAST node in document order whose page starts at or before
/// `currentPageIndex` (0-based). Null before the first paged entry (or when
/// no entry has a page). Unpaged nodes are skipped, but their children still
/// participate.
export function currentOutlinePath(nodes: OutlineNode[], currentPageIndex: number): string | null {
  let best: string | null = null;
  const walk = (list: OutlineNode[], prefix: string): void => {
    list.forEach((n, i) => {
      const path = prefix ? `${prefix}.${i}` : String(i);
      if (n.pageIndex !== null && n.pageIndex <= currentPageIndex) best = path;
      walk(n.children, path);
    });
  };
  walk(nodes, '');
  return best;
}
