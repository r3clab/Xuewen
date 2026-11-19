/// Keyboard scrolling for the reader, reachable from the global keymap.
///
/// The Stage scrolls under the pointer and on touch but binds no keys of its
/// own (EmbedPDF 3.x), so the keymap supplies the ones a native scroller
/// answers. Same division as annotations/commands.ts: PdfDeck registers the
/// lookup of the active document's Stage, PdfViewer marks the region whose
/// focus the keys belong to, and nothing here imports an @embedpdf package at
/// runtime (see the note there on why).

type Behavior = 'smooth' | 'instant';

/// The Stage capability, narrowed to what the keys drive.
export interface ScrollStage {
  scrollBy(options: { left?: number; top?: number; behavior?: Behavior }): void;
  getViewportSize(): { readonly width: number; readonly height: number };
  goToFirstPage(options?: { behavior?: Behavior }): void;
  goToLastPage(options?: { behavior?: Behavior }): void;
}

export const SCROLL_KEYS = ['ArrowDown', 'ArrowUp', 'ArrowRight', 'ArrowLeft', 'PageDown', 'PageUp', 'Home', 'End'] as const;
export type ScrollKey = (typeof SCROLL_KEYS)[number];

/// What an arrow key scrolls, as browsers size a line.
const LINE_PX = 40;
/// The share of a screen a page key moves, as Chromium's: the line at the
/// fold stays in view, read twice rather than skipped.
const PAGE_SHARE = 0.875;

let activeStage: () => ScrollStage | null = () => null;
let region: HTMLElement | null = null;

/// Publish the lookup of the active document's Stage; the returned function
/// withdraws exactly this one.
export function registerReaderStage(lookup: () => ScrollStage | null): () => void {
  activeStage = lookup;
  return () => {
    if (activeStage === lookup) activeStage = () => null;
  };
}

/// Marks the reader's own region. A key belongs to the reader while focus is
/// inside it or nowhere at all (a click on a page leaves it on the body);
/// focus anywhere else, such as the dock, keeps the browser's own scrolling.
export function readerRegion(el: HTMLElement): () => void {
  region = el;
  return () => {
    if (region === el) region = null;
  };
}

/// Whether the scroll keys act on the reader right now.
export function readerHasKeys(): boolean {
  if (region === null || activeStage() === null) return false;
  const at = document.activeElement;
  return at === null || at === document.body || region.contains(at);
}

export function scrollReader(key: ScrollKey): void {
  const stage = activeStage();
  if (!stage) return;
  const page = () => Math.max(LINE_PX, stage.getViewportSize().height * PAGE_SHARE);
  switch (key) {
    case 'ArrowDown':
      return stage.scrollBy({ top: LINE_PX, behavior: 'instant' });
    case 'ArrowUp':
      return stage.scrollBy({ top: -LINE_PX, behavior: 'instant' });
    case 'ArrowRight':
      return stage.scrollBy({ left: LINE_PX, behavior: 'instant' });
    case 'ArrowLeft':
      return stage.scrollBy({ left: -LINE_PX, behavior: 'instant' });
    case 'PageDown':
      return stage.scrollBy({ top: page(), behavior: 'smooth' });
    case 'PageUp':
      return stage.scrollBy({ top: -page(), behavior: 'smooth' });
    case 'Home':
      return stage.goToFirstPage({ behavior: 'instant' });
    case 'End':
      return stage.goToLastPage({ behavior: 'instant' });
  }
}
