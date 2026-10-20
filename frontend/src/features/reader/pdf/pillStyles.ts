import { DUR, dur, EASE } from '$shared/ui/motion';

/// Show/hide motion for the floating pills, shared so the two can't drift
/// apart (they fade together in zen). A short rise accompanies the fade —
/// a bare dissolve over a scrolling page reads as a stutter rather than as
/// motion — and the compositing layer is declared up front, or the first
/// frame of every fade is spent promoting a backdrop-filtered element.
/// Reveal is quicker than the hide: scrolling back up is a request to use
/// the toolbar, while hiding may as well get out of the way gently.
///
/// `transform` is free for this because Tailwind v4 centers the toolbar
/// with the separate `translate` property (`-translate-x-1/2`), so the two
/// don't overwrite each other.
export function pillMotionStyle(visible: boolean): string {
  const ms = dur(visible ? DUR.fast : DUR.base);
  return [
    `transition: opacity ${ms}ms ${EASE}, transform ${ms}ms ${EASE}`,
    `transform: translateY(${visible ? '0' : '-6px'})`,
    'will-change: opacity, transform',
  ].join('; ');
}

/// The reader pill's shared control styling — one copy for the components
/// whose buttons are meant to look identical (PdfToolbar, AnnotationTools,
/// AnnotationHistory, AnnotationSelectionMenu, PdfFindBar). PdfQuickActions'
/// serif glyph buttons and the side panels' denser row buttons differ on
/// purpose and keep their own.
export const btn =
  'rounded-lg p-1.5 text-stone-600 hover:bg-parchment hover:text-ink disabled:opacity-40 disabled:hover:bg-transparent dark:text-stone-300 dark:hover:bg-stone-800';
export const activeBtn =
  'rounded-lg p-1.5 bg-amber-700/10 text-amber-700 dark:bg-amber-500/15 dark:text-amber-500';
