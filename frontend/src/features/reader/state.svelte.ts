import { tick } from 'svelte';

/// Reader UI state. The find bar is per-open-paper (keyed by documentId) so the
/// global keymap reaches the active tab's bar while hidden tabs keep their own.
/// The side panel, by contrast, is a single GLOBAL setting shared across every
/// open paper: opening/closing it or switching thumbnails↔outline in one paper
/// applies to all open tabs and to every paper opened afterwards.
export type PanelTab = 'thumbs' | 'outline' | 'annotations';

export const reader = $state<{
  find: Record<string, boolean>;
  panel: PanelTab | null;
  lastPanel: PanelTab;
}>({ find: {}, panel: null, lastPanel: 'thumbs' });

/// How wide the panel is per view. Thumbnails and an outline are page numbers
/// and short headings; annotations are prose, and a quoted sentence wrapped to
/// 176px is unreadable. One place, because the width is both the spring target
/// and the fixed width of the panel inside it — those two must never disagree.
const PANEL_WIDTHS: Record<PanelTab, number> = {
  thumbs: 176,
  outline: 176,
  annotations: 264,
};

export function panelWidth(tab: PanelTab): number {
  return PANEL_WIDTHS[tab];
}

/// Open/close one document's find bar. Omit `open` to toggle.
export function setFind(id: string, open?: boolean): void {
  reader.find[id] = open ?? !reader.find[id];
}

/// The toolbar's single sidebar button (global): closed → reopen at the
/// last-used view (thumbnails on first open); open → close.
export function toggleSidebar(): void {
  reader.panel = reader.panel ? null : reader.lastPanel;
}

/// The panel's segmented control: switch the (global) open view and remember it.
export function setPanelView(tab: PanelTab): void {
  reader.panel = tab;
  reader.lastPanel = tab;
}

/// `a`: show the annotations list, or close the panel if it is already the
/// open view. Same shape as the dock's `i`/`c` — pressing the key that got you
/// somewhere takes you back — rather than plain toggleSidebar, which would
/// reopen on thumbnails and leave the reader hunting for the annotations tab.
export function toggleAnnotationsPanel(): void {
  if (reader.panel === 'annotations') reader.panel = null;
  else setPanelView('annotations');
}

/// ⌘F: open (or refocus) a document's find bar. Focus waits a tick so a
/// just-mounted bar exists; `select()` keeps a previous query editable.
export function openFind(id: string): void {
  reader.find[id] = true;
  void tick().then(() => {
    const input = document.querySelector<HTMLInputElement>(`[data-find-input="${id}"]`);
    if (input) {
      input.focus();
      input.select();
    }
  });
}

/// Forget a closed tab's find state (called from closeTab). The side panel is
/// global, so there is nothing panel-related to drop per tab.
export function dropReaderState(id: string): void {
  delete reader.find[id];
}
