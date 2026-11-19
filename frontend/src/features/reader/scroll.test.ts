import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { readerHasKeys, readerRegion, registerReaderStage, scrollReader, type ScrollStage } from './scroll';

function fakeStage(): ScrollStage {
  return {
    scrollBy: vi.fn(),
    getViewportSize: () => ({ width: 800, height: 600 }),
    goToFirstPage: vi.fn(),
    goToLastPage: vi.fn(),
  };
}

let stage: ScrollStage;
let region: HTMLElement;
let cleanup: (() => void)[];

beforeEach(() => {
  stage = fakeStage();
  region = document.createElement('div');
  const button = document.createElement('button');
  region.append(button);
  document.body.append(region);
  cleanup = [registerReaderStage(() => stage), readerRegion(region), () => region.remove()];
});

afterEach(() => {
  for (const undo of cleanup.splice(0)) undo();
});

describe('scrollReader', () => {
  it('moves a line per arrow, at once so a held key scrolls steadily', () => {
    scrollReader('ArrowDown');
    scrollReader('ArrowUp');
    scrollReader('ArrowRight');
    scrollReader('ArrowLeft');
    expect(vi.mocked(stage.scrollBy).mock.calls).toEqual([
      [{ top: 40, behavior: 'instant' }],
      [{ top: -40, behavior: 'instant' }],
      [{ left: 40, behavior: 'instant' }],
      [{ left: -40, behavior: 'instant' }],
    ]);
  });

  it('moves most of a screen per page key, keeping the line at the fold in view', () => {
    scrollReader('PageDown');
    scrollReader('PageUp');
    expect(vi.mocked(stage.scrollBy).mock.calls).toEqual([
      [{ top: 525, behavior: 'smooth' }],
      [{ top: -525, behavior: 'smooth' }],
    ]);
  });

  it('goes to the first and last page', () => {
    scrollReader('Home');
    scrollReader('End');
    expect(stage.goToFirstPage).toHaveBeenCalledWith({ behavior: 'instant' });
    expect(stage.goToLastPage).toHaveBeenCalledWith({ behavior: 'instant' });
  });
});

describe('readerHasKeys', () => {
  it('holds while focus is on the page or inside the reader', () => {
    (document.activeElement as HTMLElement | null)?.blur();
    expect(readerHasKeys()).toBe(true);
    region.querySelector('button')!.focus();
    expect(readerHasKeys()).toBe(true);
  });

  it('leaves the keys to the browser while focus is elsewhere, such as the dock', () => {
    const outside = document.createElement('button');
    document.body.append(outside);
    outside.focus();
    expect(readerHasKeys()).toBe(false);
    outside.remove();
  });

  it('is off while the active tab has no ready document', () => {
    cleanup.push(registerReaderStage(() => null));
    expect(readerHasKeys()).toBe(false);
  });

  it('is off once the reader unmounts', () => {
    const [, detachRegion] = cleanup;
    detachRegion();
    expect(readerHasKeys()).toBe(false);
  });
});
