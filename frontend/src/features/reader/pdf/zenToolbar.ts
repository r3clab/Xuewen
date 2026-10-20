/// Zen-mode auto-hide policy for the floating PDF toolbar. Pure decision
/// logic — the component owns the actual timer; this owns when it may run
/// and what visibility results.
export const HIDE_DELAY_MS = 1500;
/// Pointer within this many px of the reader's top edge re-reveals the pill.
export const HOT_ZONE_PX = 96;

export interface ToolbarHold {
  zen: boolean;
  hotZone: boolean; //     pointer inside the top hot zone
  pointerOver: boolean; // pointer over the pill itself
  focusWithin: boolean; // keyboard focus inside the pill
  findOpen: boolean; //   reader state, not a toolbar-local interaction
  localHold: boolean; //  any toolbar-local interaction (page input, open menus)
}

/// An explicit interaction with the pill, in or out of zen. Split out of
/// `holdVisible` because scroll-hide runs outside zen too, where the zen gate
/// alone reports everything as held.
export function heldOpen(s: ToolbarHold): boolean {
  return s.hotZone || s.pointerOver || s.focusWithin || s.findOpen || s.localHold;
}

/// While any hold is active the toolbar stays visible and the hide timer
/// must be cancelled. Outside zen the toolbar is unconditionally visible.
export function holdVisible(s: ToolbarHold): boolean {
  return !s.zen || heldOpen(s);
}

/// Final visibility: held, or the hide timer hasn't fired yet.
export function toolbarVisible(s: ToolbarHold, idleExpired: boolean): boolean {
  return holdVisible(s) || !idleExpired;
}

/// The center toolbar additionally yields to reading direction (see
/// pdf/scrollHide.ts) — everywhere, not just in zen. A hold outranks it, so
/// the pill can't slip away under the pointer that is using it.
export function readerToolbarVisible(s: ToolbarHold, idleExpired: boolean, scrollHidden: boolean): boolean {
  return toolbarVisible(s, idleExpired) && (heldOpen(s) || !scrollHidden);
}
