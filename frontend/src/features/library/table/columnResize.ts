import type { Action } from 'svelte/action';

export interface ColumnResizeParams {
  /// Current committed width — kept fresh across gestures via update().
  width: number;
  min: number;
  /// Container-aware ceiling, called once per gesture (it reads clientWidth,
  /// too costly to evaluate on every render just for an attribute).
  max: () => number;
  /// Which edge of its column the handle sits on. A 'left' handle (used when
  /// the column's right neighbor-boundary belongs to someone else — e.g. the
  /// last column, whose right edge is the table edge) inverts the drag:
  /// pulling the divider left grows the column. Default 'right'.
  edge?: 'left' | 'right';
  /// Keyboard resize step in px (default 8).
  step?: number;
  /// Live preview: every pointermove / keyboard step.
  onResize: (px: number) => void;
  /// Persist: once per drag (on release), once per keyboard step.
  onCommit: (px: number) => void;
  onAutoFit: () => void;
}

/// Drag / keyboard / double-click behavior for a column-resize handle
/// (same Action shape as clickOutside). Pointer capture keeps the gesture on
/// the handle even when the cursor leaves it; ArrowLeft/ArrowRight resize
/// from the keyboard; double-click asks the owner to auto-fit.
export const columnResize: Action<HTMLElement, ColumnResizeParams> = (node, params) => {
  let p = params;
  let dragging = false;
  let moved = false;
  let startX = 0;
  let startWidth = 0;
  let lastWidth = 0;
  let maxPx = Infinity;

  const clamp = (px: number) => Math.min(maxPx, Math.max(p.min, px));
  // Arrows and drags move the DIVIDER; a left-edge handle grows its column
  // by moving that divider left, hence the sign flip.
  const sign = () => (p.edge === 'left' ? -1 : 1);

  // A drag's release generates a synthetic click that browsers may retarget
  // to the common ancestor of down/up — i.e. the <th> housing a sort button.
  // A one-shot capture-phase swallower (armed only after real movement, so
  // plain clicks and dblclick still work) eats it regardless of target.
  const swallowClick = (e: MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
  };

  // Pinning the cursor for the whole gesture stops it flickering while
  // crossing sibling cells; killing user-select stops mid-drag selection.
  const restore = () => {
    document.documentElement.style.cursor = '';
    document.documentElement.style.userSelect = '';
  };

  function onPointerDown(e: PointerEvent) {
    if (e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    dragging = true;
    moved = false;
    startX = e.clientX;
    startWidth = p.width;
    lastWidth = p.width;
    maxPx = p.max();
    try {
      node.setPointerCapture(e.pointerId);
    } catch {
      /* no pointer capture (jsdom) — plain bubbling still works */
    }
    document.documentElement.style.cursor = 'col-resize';
    document.documentElement.style.userSelect = 'none';
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging) return;
    const dx = (e.clientX - startX) * sign();
    if (Math.abs(dx) > 3) moved = true;
    lastWidth = clamp(startWidth + dx);
    p.onResize(lastWidth);
  }

  function onPointerEnd(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    try {
      node.releasePointerCapture(e.pointerId);
    } catch {
      /* not captured */
    }
    restore();
    p.onCommit(lastWidth);
    if (moved) window.addEventListener('click', swallowClick, { capture: true, once: true });
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    e.stopPropagation();
    maxPx = p.max();
    const step = p.step ?? 8;
    const next = clamp(p.width + (e.key === 'ArrowRight' ? step : -step) * sign());
    p.onResize(next);
    p.onCommit(next);
  }

  function onDblClick(e: MouseEvent) {
    e.stopPropagation();
    p.onAutoFit();
  }

  node.addEventListener('pointerdown', onPointerDown);
  node.addEventListener('pointermove', onPointerMove);
  node.addEventListener('pointerup', onPointerEnd);
  node.addEventListener('pointercancel', onPointerEnd);
  node.addEventListener('keydown', onKeydown);
  node.addEventListener('dblclick', onDblClick);

  return {
    update(next: ColumnResizeParams) {
      p = next;
    },
    destroy() {
      node.removeEventListener('pointerdown', onPointerDown);
      node.removeEventListener('pointermove', onPointerMove);
      node.removeEventListener('pointerup', onPointerEnd);
      node.removeEventListener('pointercancel', onPointerEnd);
      node.removeEventListener('keydown', onKeydown);
      node.removeEventListener('dblclick', onDblClick);
      // An unmount mid-drag must not leave the page stuck in col-resize.
      if (dragging) restore();
    },
  };
};
