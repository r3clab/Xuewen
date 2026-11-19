import type { Attachment } from 'svelte/attachments';
import { anchorScrollHide, INITIAL_SCROLL_HIDE, nextScrollHide, type ScrollHide } from './scrollHide';
import { reader } from '$features/reader/state.svelte';
import { viewer } from '$features/reader/tabs/state.svelte';
import {
  HIDE_DELAY_MS,
  HOT_ZONE_PX,
  holdVisible,
  readerToolbarVisible,
  toolbarVisible,
  type ToolbarHold,
} from './zenToolbar';

/// Shared zen auto-hide for the reader's floating pills (center toolbar +
/// top-right quick actions). One instance per PdfDocument tab: it owns the DOM
/// signals and the hide timer; the visibility decision stays in the pure
/// pdf/zenToolbar.ts. Both pills bind the same handlers, so hovering or
/// focusing either one holds both visible and they fade together.
///
/// The center toolbar reads `toolbarVisible` instead of `visible`: it also
/// hides while the reader scrolls forward, in zen or not (pdf/scrollHide.ts).
/// The quick actions stay put — they are the way out of zen.
export interface PillHide {
  readonly visible: boolean;
  readonly toolbarVisible: boolean;
  /// `{@attach pill.host}` on the element the pills float in; the top-edge
  /// hot zone is measured from it.
  readonly host: Attachment<HTMLElement>;
  onWindowMove(e: PointerEvent): void;
  onScroll(scrollTop: number): void;
  /// A scroll the reader did not drive (jump to page, find, outline): it moves
  /// the anchor so the jump isn't read as reading direction.
  onScrollJump(scrollTop: number): void;
  pillEnter(): void;
  pillLeave(): void;
  focusIn(): void;
  focusOut(): void;
}

/// MUST be called during component init — it registers $effects.
/// `localHold` is the toolbar's own interactions (page editing, an open
/// menu); it is read inside a `$derived`, so its reactive sources are
/// tracked.
export function createPillHide(getDocumentId: () => string, localHold: () => boolean): PillHide {
  let hotZone = $state(false);
  let pointerOver = $state(false);
  let focusWithin = $state(false);
  let idleExpired = $state(false);
  // Only the decision is reactive. The anchor moves on most scroll events
  // (it tracks the extreme of the current run), and putting the whole record
  // in $state re-ran the derived visibility on every frame of every scroll.
  let scroll: ScrollHide = INITIAL_SCROLL_HIDE;
  let scrollHidden = $state(false);
  let hostEl: HTMLElement | null = null;

  function applyScroll(next: ScrollHide): void {
    scroll = next;
    if (next.hidden !== scrollHidden) scrollHidden = next.hidden;
  }

  const hold = $derived<ToolbarHold>({
    zen: viewer.zen,
    hotZone,
    pointerOver,
    focusWithin,
    findOpen: !!reader.find[getDocumentId()],
    localHold: localHold(),
  });
  const visible = $derived(toolbarVisible(hold, idleExpired));
  const toolbar = $derived(readerToolbarVisible(hold, idleExpired, scrollHidden));

  // Any hold cancels the countdown and re-arms visibility; once every hold
  // drops in zen, the countdown starts.
  $effect(() => {
    if (holdVisible(hold)) {
      idleExpired = false;
      return;
    }
    const t = setTimeout(() => (idleExpired = true), HIDE_DELAY_MS);
    return () => clearTimeout(t);
  });

  return {
    get visible() {
      return visible;
    },
    get toolbarVisible() {
      return toolbar;
    },
    host(el) {
      hostEl = el;
      return () => {
        if (hostEl === el) hostEl = null;
      };
    },
    // Window-level so it works while the pills are faded out; only the
    // active tab's controller reacts (hidden tabs stay mounted). Tracked in
    // and out of zen, since scroll-hide needs the same top-edge reveal.
    onWindowMove(e) {
      if (viewer.activeId !== getDocumentId()) return;
      if (!hostEl) return;
      const top = hostEl.getBoundingClientRect().top;
      hotZone = e.clientY >= top && e.clientY - top < HOT_ZONE_PX;
    },
    onScroll(scrollTop) {
      applyScroll(nextScrollHide(scroll, scrollTop));
    },
    onScrollJump(scrollTop) {
      applyScroll(anchorScrollHide(scroll, scrollTop));
    },
    pillEnter() {
      pointerOver = true;
    },
    pillLeave() {
      pointerOver = false;
    },
    focusIn() {
      focusWithin = true;
    },
    focusOut() {
      focusWithin = false;
    },
  };
}
