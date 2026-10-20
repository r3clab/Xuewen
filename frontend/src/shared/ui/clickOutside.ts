import type { Action } from 'svelte/action';

/// Window-level "pointerdown outside dismisses" shared by menus/popovers:
/// any pointerdown whose target is not inside the node calls the callback.
/// Attach to the popover element itself when it's `{#if open}`-gated (the
/// listener then lives only while open — and the opening click can't
/// self-dismiss, since it fires before the element mounts); on an
/// always-mounted wrapper, guard the callback with the open flag instead.
export const clickOutside: Action<HTMLElement, () => void> = (node, onOutside) => {
  let cb = onOutside;
  const handler = (e: PointerEvent) => {
    if (e.target instanceof Node && node.contains(e.target)) return;
    cb();
  };
  window.addEventListener('pointerdown', handler);
  return {
    update(next: () => void) {
      cb = next;
    },
    destroy() {
      window.removeEventListener('pointerdown', handler);
    },
  };
};
