import { describe, expect, it, vi } from 'vitest';
import { closeOnScroll } from './closeOnScroll';

describe('closeOnScroll', () => {
  it('hears a scroll in an inner pane, which does not bubble, until detached', () => {
    const close = vi.fn();
    const pane = document.createElement('div');
    document.body.append(pane);
    const detach = closeOnScroll(close)();
    pane.dispatchEvent(new Event('scroll'));
    expect(close).toHaveBeenCalledTimes(1);
    detach();
    pane.dispatchEvent(new Event('scroll'));
    expect(close).toHaveBeenCalledTimes(1);
    pane.remove();
  });
});
