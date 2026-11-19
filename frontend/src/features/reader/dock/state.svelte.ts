import { flushSync } from 'svelte';
import { readLocalJson, writeLocal } from '$shared/browser/persist';
import { FocusTarget } from '$shared/ui/focus';
import { viewer } from '$features/reader/tabs/state.svelte';

/// The reader dock: one right-docked panel carrying the paper's record and
/// its Ask thread on a single surface, with the composer pinned to its foot.
/// `open` is remembered across sessions.
export const dock = $state<{ open: boolean }>({ open: false });

/// The dock's composer, for `c`, the rail's 問 and "Ask about this".
export const composerFocus = new FocusTarget();

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

export function openDock(): void {
  dock.open = true;
  saveDock();
}

export function closeDock(): void {
  dock.open = false;
  saveDock();
}

/// `i`: the panel's own toggle. The dock only exists over a PDF.
export function toggleDock(): void {
  if (viewer.activeId === null) return;
  if (dock.open) closeDock();
  else openDock();
}

/// `c`: open the dock if needed and put the cursor in the composer — with the
/// panel already open this only moves focus, never closes the thing it was
/// asked to type into.
export function askInDock(): void {
  if (viewer.activeId === null) return;
  openDock();
  flushSync();
  composerFocus.focus();
}
