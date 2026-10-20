/// Scroll-direction policy for the reader's floating toolbar: reading
/// forward hides it, scrolling back reveals it. Pure decision logic — the
/// controller (pdf/pillHide.svelte.ts) feeds it scroll offsets and owns the
/// resulting state.

/// Sustained travel in one direction before visibility flips. Below this a
/// reversal only re-anchors, so trackpad jitter and rubber-band overscroll
/// can't strobe the toolbar.
export const SCROLL_HIDE_PX = 48;
/// The top of the document always shows the toolbar, however the reader got there.
export const TOP_EPS_PX = 8;

export interface ScrollHide {
  hidden: boolean;
  /// Offset the current run of travel is measured from; null until the first
  /// observed scroll, so a tab restored deep in a document doesn't read its
  /// opening offset as one huge forward scroll.
  anchor: number | null;
}

export const INITIAL_SCROLL_HIDE: ScrollHide = { hidden: false, anchor: null };

/// Re-anchor without judging direction — for scrolls the reader didn't drive
/// (programmatic jumps), which must not hide the toolbar.
export function anchorScrollHide(s: ScrollHide, scrollTop: number): ScrollHide {
  return { ...s, anchor: scrollTop };
}

export function nextScrollHide(s: ScrollHide, scrollTop: number): ScrollHide {
  if (scrollTop <= TOP_EPS_PX) return { hidden: false, anchor: scrollTop };
  if (s.anchor === null) return anchorScrollHide(s, scrollTop);
  const travel = scrollTop - s.anchor;
  if (s.hidden ? travel <= -SCROLL_HIDE_PX : travel >= SCROLL_HIDE_PX) {
    return { hidden: !s.hidden, anchor: scrollTop };
  }
  // A reversal that hasn't earned a flip yet still re-anchors, or the
  // threshold would be measured from a stale high-water mark.
  const reversed = s.hidden ? travel > 0 : travel < 0;
  return reversed ? anchorScrollHide(s, scrollTop) : s;
}
