import { PINNED_COLUMNS, PINNED_KEYS, type PinnedColumnKey } from './columns';

/// Persisted library-table column widths (px). Same localStorage shape as
/// `dock` (reader/dock/state.svelte.ts) / `pdfAppearance` (theme.svelte.ts),
/// in its own module because only LibraryTable reads it. One deliberate
/// difference: a drag fires pointermove at 60–120Hz, so the live update
/// (`setColumnWidth`, in-memory only) and the storage write
/// (`commitColumnWidth`, once per gesture) are split instead of
/// mutate-and-save-together.

const STORAGE_KEY = 'xuewen-library-columns';
/// Bump only for shape-level breaking changes; adding/removing/renaming a
/// column is already handled by initColumnWidths' per-key reconciliation.
const SCHEMA_VERSION = 1;

function defaults(): Record<PinnedColumnKey, number> {
  return Object.fromEntries(
    PINNED_KEYS.map((k) => [k, PINNED_COLUMNS[k].defaultWidth]),
  ) as Record<PinnedColumnKey, number>;
}

export const columnWidths = $state<Record<PinnedColumnKey, number>>(defaults());

/// Load the remembered widths. Call once at startup. Reconciles per key:
/// unknown keys in storage are ignored, missing/out-of-range values keep
/// that key's default — a blob stored by an older column set degrades
/// gracefully instead of needing a migration.
export function initColumnWidths(): void {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return;
    const v = JSON.parse(raw) as Record<string, unknown>;
    if (v.v !== SCHEMA_VERSION) return;
    for (const key of PINNED_KEYS) {
      const px = v[key];
      const { minWidth, maxWidth } = PINNED_COLUMNS[key];
      if (typeof px === 'number' && Number.isFinite(px) && px >= minWidth && px <= maxWidth) {
        columnWidths[key] = Math.round(px);
      }
    }
  } catch {
    /* corrupted value — keep defaults */
  }
}

function persistColumnWidths(): void {
  try {
    localStorage.setItem(
      STORAGE_KEY,
      JSON.stringify({ v: SCHEMA_VERSION, ...$state.snapshot(columnWidths) }),
    );
  } catch {
    /* no localStorage — widths still apply, only persistence is lost */
  }
}

function clampTo(key: PinnedColumnKey, px: number): number {
  const { minWidth, maxWidth } = PINNED_COLUMNS[key];
  return Math.min(maxWidth, Math.max(minWidth, Math.round(px)));
}

/// Live update during a drag: in-memory only, no I/O.
export function setColumnWidth(key: PinnedColumnKey, px: number): void {
  columnWidths[key] = clampTo(key, px);
}

/// Final width of a gesture (drag release, keyboard step, auto-fit one).
export function commitColumnWidth(key: PinnedColumnKey, px: number): void {
  setColumnWidth(key, px);
  persistColumnWidths();
}

/// Batch commit (auto-fit all): one storage write for several columns.
export function commitColumnWidths(widths: Partial<Record<PinnedColumnKey, number>>): void {
  for (const key of PINNED_KEYS) {
    const px = widths[key];
    if (px !== undefined) setColumnWidth(key, px);
  }
  persistColumnWidths();
}

export function resetColumnWidths(): void {
  Object.assign(columnWidths, defaults());
  persistColumnWidths();
}
