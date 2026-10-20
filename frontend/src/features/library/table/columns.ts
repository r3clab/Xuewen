/// Layout metadata for the library table's columns. The markup in
/// LibraryTable.svelte stays hand-written per column (checkbox, star, sort
/// buttons, and plain labels are too heterogeneous to generalize); this
/// module is only the single source of truth for widths — and the budget
/// arithmetic composed from them — so the colgroup, drag-resize, auto-fit,
/// and persistence all agree on one set of numbers.

/// The seven columns with a user-adjustable width. The two icon columns
/// (checkbox, star) are fixed, and Tags deliberately has no width at all:
/// under `table-layout: fixed` the one width-less column soaks up whatever
/// the pinned columns leave, which is what keeps the table filling the pane
/// with zero JS reflow work.
export type PinnedColumnKey =
  | 'name'
  | 'title'
  | 'firstAuthor'
  | 'lastAuthor'
  | 'venue'
  | 'year'
  | 'added';

export interface ColumnDef {
  label: string;
  defaultWidth: number;
  minWidth: number;
  maxWidth: number;
}

/// maxWidth is a sanity cap on stored/derived values, not the live layout
/// bound — during a drag or auto-fit the real ceiling is the container-aware
/// maxFor() in LibraryTable, so wide windows can use genuinely wide columns.
/// Text-heavy columns get generous caps; Year/Added stay tight because extra
/// width there is only empty space.
export const PINNED_COLUMNS: Record<PinnedColumnKey, ColumnDef> = {
  name: { label: 'Name', defaultWidth: 110, minWidth: 64, maxWidth: 320 },
  title: { label: 'Title', defaultWidth: 320, minWidth: 140, maxWidth: 1200 },
  firstAuthor: { label: 'First author', defaultWidth: 130, minWidth: 70, maxWidth: 560 },
  lastAuthor: { label: 'Last author', defaultWidth: 130, minWidth: 70, maxWidth: 560 },
  venue: { label: 'Venue', defaultWidth: 110, minWidth: 60, maxWidth: 320 },
  year: { label: 'Year', defaultWidth: 64, minWidth: 56, maxWidth: 100 },
  added: { label: 'Added', defaultWidth: 112, minWidth: 90, maxWidth: 180 },
};

export const PINNED_KEYS = Object.keys(PINNED_COLUMNS) as PinnedColumnKey[];

/// The w-9 checkbox/star columns (2.25rem).
export const ICON_COLUMN_PX = 36;

/// Space always reserved for the flexible Tags column when clamping a
/// resize or auto-fit, so no gesture can push the table into horizontal
/// scroll on its own.
export const TAGS_MIN_PX = 120;

/// What auto-fit-all leaves for Tags when growing columns into surplus
/// space — a comfortable strip, unlike TAGS_MIN_PX, which is only the hard
/// floor a drag may squeeze Tags down to.
export const TAGS_TARGET_PX = 280;

/// Cell padding (px-3 on both sides = 24px) plus a small buffer, added on
/// top of the measured text when auto-fitting.
export const AUTO_FIT_PADDING = 32;

// --- width-budget arithmetic ---
// The table is exactly chrome + pinned + Tags. That invariant lives HERE,
// once: LibraryTable feeds in the DOM reads (pane width, current widths) and
// every clamp/floor/budget below composes from the same formula.

/// The fixed chrome flanking the pinned columns: the two icon columns
/// (checkbox, star).
export function chromePx(): number {
  return 2 * ICON_COLUMN_PX;
}

/// The floor below which the flexible Tags column would collapse to zero
/// and wrap: chrome + every pinned width + the Tags reserve.
export function tableMinWidth(widths: Record<PinnedColumnKey, number>): number {
  return chromePx() + PINNED_KEYS.reduce((s, k) => s + widths[k], 0) + TAGS_MIN_PX;
}

/// Ceiling for dragging/auto-fitting ONE column: it may grow while every
/// other pinned column keeps its width and Tags keeps its reserve — what
/// keeps the table filling the pane with no horizontal scroll. Falls back
/// to the static maxWidth cap when layout isn't available (paneWidth <= 0,
/// e.g. jsdom).
export function dragCeiling(
  key: PinnedColumnKey,
  widths: Record<PinnedColumnKey, number>,
  paneWidth: number,
): number {
  const stat = PINNED_COLUMNS[key].maxWidth;
  if (paneWidth <= 0) return stat;
  const others = PINNED_KEYS.reduce((s, k) => (k === key ? s : s + widths[k]), 0);
  return Math.max(
    PINNED_COLUMNS[key].minWidth,
    Math.min(stat, paneWidth - chromePx() - others - TAGS_MIN_PX),
  );
}

/// What auto-fit-all may distribute across the pinned columns: the pane
/// minus chrome, leaving Tags its comfortable TAGS_TARGET_PX strip.
export function autoFitBudget(paneWidth: number): number {
  return paneWidth - chromePx() - TAGS_TARGET_PX;
}
