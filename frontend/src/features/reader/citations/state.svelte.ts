import type { Reference } from './references';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

export interface HoveredCitation {
  reference: Reference;
  matchedPaper: PaperSummary | null;
  screenX: number;
  screenY: number;
}

// Single global hovered-citation slot (only one popover at a time).
export const citationHover = $state<{ current: HoveredCitation | null }>({ current: null });

let hideTimer: ReturnType<typeof setTimeout> | null = null;

export function showCitation(c: HoveredCitation): void {
  if (hideTimer) {
    clearTimeout(hideTimer);
    hideTimer = null;
  }
  citationHover.current = c;
}

/** Hide after a short grace delay so the pointer can travel into the popover. */
export function hideCitationSoon(): void {
  if (hideTimer) clearTimeout(hideTimer);
  hideTimer = setTimeout(() => {
    citationHover.current = null;
    hideTimer = null;
  }, 120);
}

export function cancelHideCitation(): void {
  if (hideTimer) {
    clearTimeout(hideTimer);
    hideTimer = null;
  }
}

/** Hide immediately (an action was taken on the reference). Clears any
 *  pending grace timer too — writes to `citationHover.current` must come
 *  through this module so the timer bookkeeping stays consistent. */
export function hideCitationNow(): void {
  if (hideTimer) {
    clearTimeout(hideTimer);
    hideTimer = null;
  }
  citationHover.current = null;
}
