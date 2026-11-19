<!--
  The one cursor-anchored `role="menu"` shell behind PaperContextMenu,
  ColumnHeaderMenu, and the pill bar's PillMenu: container styling, viewport
  clamping, click-outside/Escape/scroll dismissal, roving item focus, and
  focus capture/restore live here once. Callers `{#if}`-gate the shell — mount
  IS the open transition — and keep only their action content and mode
  machinery (what the menus genuinely differ in).
-->
<script lang="ts">
  import type { Snippet } from 'svelte';
  import type { Attachment } from 'svelte/attachments';
  import { clickOutside } from './clickOutside';
  import { closeOnScroll } from './closeOnScroll';
  import { escapeLayer } from './escape';
  import { returnFocus } from './focus';
  import { menuItems, menuNavKeydown } from './menuNav';
  import { clampMenuPosition } from './popoverPosition';

  let {
    x,
    y,
    label,
    width,
    dismissOnScroll = true,
    onClose,
    onEscape,
    children,
  }: {
    x: number;
    y: number;
    label: string;
    /// Width utility class (w-36/w-44/w-48) — the one style the menus vary in.
    width: string;
    /// A menu anchored inside a scrolling pane should dismiss when that pane
    /// scrolls under it (native context-menu behavior); one anchored to
    /// static chrome (the pill bar) opts out, or an unrelated scroll would
    /// discard an in-progress rename.
    dismissOnScroll?: boolean;
    onClose: () => void;
    /// Escape falls through to onClose unless the caller needs its own
    /// semantics (PillMenu steps back one level instead of closing).
    onEscape?: () => void;
    children: Snippet;
  } = $props();

  let left = $state(0);
  let top = $state(0);

  // Clamped so a click near the right/bottom viewport edge doesn't render the
  // menu off-screen, again whenever the content changes height (action list →
  // rename → delete confirm), and with its first action focused. Re-runs when
  // the anchor moves: right-clicking a second target retargets a
  // still-mounted menu, which must behave exactly like a fresh open.
  function anchoredAt(x: number, y: number): Attachment<HTMLElement> {
    return (menu) => {
      const place = () => {
        const p = clampMenuPosition(x, y, menu);
        left = p.left;
        top = p.top;
      };
      place();
      menuItems(menu)[0]?.focus();
      const ro = new ResizeObserver(place);
      ro.observe(menu);
      return () => ro.disconnect();
    };
  }

</script>

<!-- Window blur rides along with scroll dismissal: such a menu shouldn't
     outlive a switch to another app. -->
<svelte:window onblur={() => dismissOnScroll && onClose()} />

<!-- Dismiss on any pointerdown outside the menu. The right-click that
     opened it fires its pointerdown BEFORE the menu mounts (and with it
     the action's listener) — no immediate re-close. -->
<div
  {@attach returnFocus}
  {@attach anchoredAt(x, y)}
  {@attach escapeLayer(() => (onEscape ?? onClose)())}
  {@attach dismissOnScroll && closeOnScroll(onClose)}
  use:clickOutside={onClose}
  role="menu"
  aria-label={label}
  tabindex="-1"
  onkeydown={(e) => menuNavKeydown(e.currentTarget, e)}
  class={`fixed z-50 ${width} rounded-xl border border-stone-200 bg-paper/95 p-1.5 shadow-lg backdrop-blur dark:border-stone-800 dark:bg-soot/95`}
  style={`left:${left}px;top:${top}px`}
>
  {@render children()}
</div>
