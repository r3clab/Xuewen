import { describe, expect, it, vi } from 'vitest';
import { dismissTopLayer, modalOpen, pushLayer } from './escape';

const esc = () => new KeyboardEvent('keydown', { key: 'Escape', cancelable: true });

describe('the Esc layer stack', () => {
  it('closes only the innermost layer, then the next one down', () => {
    const outer = vi.fn();
    const inner = vi.fn();
    const popOuter = pushLayer(outer);
    const popInner = pushLayer(inner);

    const first = esc();
    expect(dismissTopLayer(first)).toBe(true);
    expect(first.defaultPrevented).toBe(true);
    expect(inner).toHaveBeenCalledTimes(1);
    expect(outer).not.toHaveBeenCalled();

    popInner();
    expect(dismissTopLayer(esc())).toBe(true);
    expect(outer).toHaveBeenCalledTimes(1);
    popOuter();
  });

  it('leaves Esc untouched when nothing is open, and every other key always', () => {
    const e = esc();
    expect(dismissTopLayer(e)).toBe(false);
    expect(e.defaultPrevented).toBe(false);
    const close = vi.fn();
    const pop = pushLayer(close);
    expect(dismissTopLayer(new KeyboardEvent('keydown', { key: 'a' }))).toBe(false);
    expect(close).not.toHaveBeenCalled();
    pop();
  });

  it('skips a layer that is not active right now', () => {
    const below = vi.fn();
    const hidden = vi.fn();
    const popBelow = pushLayer(below);
    const popHidden = pushLayer(hidden, { active: () => false });
    dismissTopLayer(esc());
    expect(hidden).not.toHaveBeenCalled();
    expect(below).toHaveBeenCalledTimes(1);
    popHidden();
    popBelow();
  });

  it('reports a modal layer while one is registered', () => {
    const popMenu = pushLayer(() => {});
    expect(modalOpen()).toBe(false);
    const popDialog = pushLayer(() => {}, { modal: true });
    expect(modalOpen()).toBe(true);
    popDialog();
    expect(modalOpen()).toBe(false);
    popMenu();
  });
});
