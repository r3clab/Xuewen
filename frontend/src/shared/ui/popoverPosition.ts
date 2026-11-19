import type { Attachment } from 'svelte/attachments';

const MARGIN = 8;

/// `{@attach aboveAnchor(x, y, belowOffset)}` on a `position: fixed` popover
/// pointing at a screen point (citation hover, translate selection). It sits
/// ABOVE the point, or `belowOffset` px BELOW it when the popover's measured
/// height doesn't fit above, and is clamped horizontally to the viewport.
/// Re-placed whenever its size changes (a translation or a parse landing).
export function aboveAnchor(x: number, y: number, belowOffset: number): Attachment<HTMLElement> {
  return (popover) => {
    const place = () => {
      const { offsetWidth: width, offsetHeight: height } = popover;
      const above = y - MARGIN - height;
      popover.style.left = `${Math.max(MARGIN, Math.min(x, window.innerWidth - width - MARGIN))}px`;
      popover.style.top = `${above >= MARGIN ? above : y + belowOffset}px`;
    };
    place();
    const observer = new ResizeObserver(place);
    observer.observe(popover);
    return () => observer.disconnect();
  };
}

/// Clamp a cursor-anchored, already-measured menu so a click near the
/// right/bottom viewport edge doesn't render it off-screen (wired once in
/// ContextMenuShell).
export function clampMenuPosition(
  x: number,
  y: number,
  menu: { offsetWidth: number; offsetHeight: number },
): { left: number; top: number } {
  return {
    left: Math.min(x, window.innerWidth - menu.offsetWidth - MARGIN),
    top: Math.min(y, window.innerHeight - menu.offsetHeight - MARGIN),
  };
}
