import type { ActionReturn } from 'svelte/action';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { columnResize, type ColumnResizeParams } from './columnResize';

// jsdom's PointerEvent support has varied by version; fall back to MouseEvent
// (the handlers only read clientX/button, and pointer capture is try/caught).
function pointer(type: string, clientX: number): Event {
  const Ctor: typeof MouseEvent =
    typeof PointerEvent === 'undefined' ? MouseEvent : (PointerEvent as unknown as typeof MouseEvent);
  return new Ctor(type, { clientX, button: 0, bubbles: true, cancelable: true });
}

function makeHandle(over: Partial<ColumnResizeParams> = {}) {
  const node = document.createElement('div');
  document.body.appendChild(node);
  const params: ColumnResizeParams = {
    width: 200,
    min: 100,
    max: () => 400,
    onResize: vi.fn(),
    onCommit: vi.fn(),
    onAutoFit: vi.fn(),
    ...over,
  };
  const ret = columnResize(node, params) as ActionReturn<ColumnResizeParams>;
  return { node, params, ret };
}

function drag(node: HTMLElement, from: number, to: number) {
  node.dispatchEvent(pointer('pointerdown', from));
  node.dispatchEvent(pointer('pointermove', to));
  node.dispatchEvent(pointer('pointerup', to));
}

afterEach(() => {
  // jsdom never fires the post-drag click a browser would, so drain any
  // armed one-shot swallower before the next test's clicks.
  window.dispatchEvent(new MouseEvent('click'));
  document.body.innerHTML = '';
  document.documentElement.style.cursor = '';
  document.documentElement.style.userSelect = '';
});

describe('columnResize', () => {
  it('resizes live by the pointer delta and commits once on release', () => {
    const { node, params } = makeHandle();
    drag(node, 100, 150);
    expect(params.onResize).toHaveBeenLastCalledWith(250);
    expect(params.onCommit).toHaveBeenCalledExactlyOnceWith(250);
  });

  it('clamps to [min, max()]', () => {
    const { node, params } = makeHandle();
    node.dispatchEvent(pointer('pointerdown', 100));
    node.dispatchEvent(pointer('pointermove', 500)); // 200 + 400 → over max
    expect(params.onResize).toHaveBeenLastCalledWith(400);
    node.dispatchEvent(pointer('pointermove', -100)); // 200 - 200 → under min
    expect(params.onResize).toHaveBeenLastCalledWith(100);
    node.dispatchEvent(pointer('pointerup', -100));
    expect(params.onCommit).toHaveBeenCalledExactlyOnceWith(100);
  });

  it('restores the document cursor/user-select on release', () => {
    const { node } = makeHandle();
    node.dispatchEvent(pointer('pointerdown', 100));
    expect(document.documentElement.style.cursor).toBe('col-resize');
    node.dispatchEvent(pointer('pointerup', 100));
    expect(document.documentElement.style.cursor).toBe('');
    expect(document.documentElement.style.userSelect).toBe('');
  });

  it('swallows exactly the click following a real drag', () => {
    const { node } = makeHandle();
    const btn = document.createElement('button');
    const onClick = vi.fn();
    btn.addEventListener('click', onClick);
    document.body.appendChild(btn);

    drag(node, 100, 150);
    btn.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    expect(onClick).not.toHaveBeenCalled(); // the drag's synthetic click
    btn.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    expect(onClick).toHaveBeenCalledOnce(); // one-shot: the next click lands
  });

  it('a stationary press-and-release does not swallow the next click', () => {
    const { node } = makeHandle();
    const btn = document.createElement('button');
    const onClick = vi.fn();
    btn.addEventListener('click', onClick);
    document.body.appendChild(btn);

    drag(node, 100, 101); // within the 3px threshold
    btn.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    expect(onClick).toHaveBeenCalledOnce();
  });

  it('a left-edge handle inverts the drag: pulling left grows the column', () => {
    const { node, params } = makeHandle({ edge: 'left' });
    drag(node, 100, 50);
    expect(params.onCommit).toHaveBeenCalledExactlyOnceWith(250);
  });

  it('ArrowRight/ArrowLeft step the width and commit each press', () => {
    const { node, params } = makeHandle();
    node.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    expect(params.onResize).toHaveBeenLastCalledWith(208);
    expect(params.onCommit).toHaveBeenLastCalledWith(208);
    node.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }));
    expect(params.onCommit).toHaveBeenLastCalledWith(192); // width prop still 200
  });

  it('double-click calls onAutoFit', () => {
    const { node, params } = makeHandle();
    node.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    expect(params.onAutoFit).toHaveBeenCalledOnce();
  });

  it('destroy removes the listeners', () => {
    const { node, params, ret } = makeHandle();
    ret.destroy?.();
    drag(node, 100, 150);
    expect(params.onResize).not.toHaveBeenCalled();
    expect(params.onCommit).not.toHaveBeenCalled();
  });
});
