<!--
  The one cursor-anchored `role="menu"` shell behind PaperContextMenu,
  ColumnHeaderMenu, and the pill bar's PillMenu: container styling, viewport
  clamping, click-outside/Escape/scroll dismissal, roving item focus, and
  focus capture/restore live here once. Callers `{#if}`-gate the shell — mount
  IS the open transition — and keep only their action content and mode
  machinery (what the menus genuinely differ in).
-->
<script lang="ts">
  import { onMount, tick, type Snippet } from 'svelte';
  import { clickOutside } from './clickOutside';
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

  let menuEl = $state<HTMLDivElement | null>(null);
  let left = $state(0);
  let top = $state(0);

  // Focus moves onto the first action on open (WAI menu pattern) and back to
  // wherever it was on close — right-click doesn't move DOM focus by itself,
  // so without this, Escape/arrows would land on the page underneath.
  let prevFocus: HTMLElement | null = null;
  onMount(() => {
    prevFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    return () => prevFocus?.focus();
  });

  // Re-runs when the anchor moves: right-clicking a second target retargets
  // a still-mounted menu (no close in between), and its first action must be
  // focused again just like on a fresh open.
  $effect(() => {
    void x;
    void y;
    void tick().then(() => menuItems(menuEl)[0]?.focus());
  });

  function reclamp() {
    if (!menuEl) return;
    const p = clampMenuPosition(x, y, menuEl);
    left = p.left;
    top = p.top;
  }

  // Clamp so a click near the right/bottom viewport edge doesn't render the
  // menu off-screen; re-runs when the anchor moves.
  $effect(() => reclamp());

  // Re-clamp when the content swaps height (action list → rename → delete
  // confirm) without the caller announcing its mode changes. Guarded: jsdom
  // has no ResizeObserver, and offsetHeight is 0 there anyway.
  $effect(() => {
    if (!menuEl || typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => reclamp());
    ro.observe(menuEl);
    return () => ro.disconnect();
  });

  // CAPTURE phase is load-bearing: the app scrolls in inner overflow panes
  // (the table pane), and element scroll events don't bubble — a
  // bubble-phase window listener would never hear the scrolls that actually
  // move the menu's anchor. Window blur rides along: a menu shouldn't
  // outlive a switch to another app.
  $effect(() => {
    if (!dismissOnScroll) return;
    const dismiss = () => onClose();
    window.addEventListener('scroll', dismiss, true);
    window.addEventListener('blur', dismiss);
    return () => {
      window.removeEventListener('scroll', dismiss, true);
      window.removeEventListener('blur', dismiss);
    };
  });

  function onWindowKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') (onEscape ?? onClose)();
  }
</script>

<svelte:window onkeydown={onWindowKeydown} />

<!-- Dismiss on any pointerdown outside the menu. The right-click that
     opened it fires its pointerdown BEFORE the menu mounts (and with it
     the action's listener) — no immediate re-close. -->
<div
  bind:this={menuEl}
  use:clickOutside={onClose}
  role="menu"
  aria-label={label}
  tabindex="-1"
  onkeydown={(e) => menuNavKeydown(menuEl, e)}
  class={`fixed z-50 ${width} rounded-xl border border-stone-200 bg-paper/95 p-1.5 shadow-lg backdrop-blur dark:border-stone-800 dark:bg-soot/95`}
  style={`left:${left}px;top:${top}px`}
>
  {@render children()}
</div>
