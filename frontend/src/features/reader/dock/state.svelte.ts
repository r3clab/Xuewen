import { readLocalJson, writeLocal } from '$shared/browser/persist';
import { viewer } from '$features/reader/tabs/state.svelte';

/// Where the panel should land when it opens. Not a mode: the dock is one
/// scroll (the record, then the conversation) with the composer pinned to its
/// foot, so this only picks what gets focus or scrolled on the way in.
export type DockEntry = 'record' | 'ask';

/// The reader dock: one right-docked panel carrying the paper's record and
/// its Ask thread on a single surface. `entry` is a one-shot request that
/// ReaderDock consumes and clears; only `open` is remembered across sessions.
export const dock = $state<{ open: boolean; entry: DockEntry | null }>({ open: false, entry: null });

const DOCK_KEY = 'xuewen-dock';

/// Load the remembered dock state (default: closed). Call once at startup.
export function initDock(): void {
  const v = readLocalJson(DOCK_KEY);
  if (!v || typeof v !== 'object') return; // absent or corrupted — keep defaults
  const { open } = v as { open?: unknown };
  dock.open = open === true;
}

function saveDock(): void {
  writeLocal(DOCK_KEY, JSON.stringify({ open: dock.open }));
}

export function openDock(entry: DockEntry = 'record'): void {
  dock.open = true;
  dock.entry = entry;
  saveDock();
}

export function closeDock(): void {
  dock.open = false;
  dock.entry = null;
  saveDock();
}

/// The `i`/`c` shortcut behavior. `i` is the panel's own toggle; `c` asks for
/// the composer, so with the panel already open it moves focus there rather
/// than closing the thing it was asked to type into. The dock only exists
/// over a PDF.
export function toggleDock(entry: DockEntry = 'record'): void {
  if (viewer.activeId === null) return;
  if (!dock.open) openDock(entry);
  else if (entry === 'ask') dock.entry = 'ask';
  else closeDock();
}
