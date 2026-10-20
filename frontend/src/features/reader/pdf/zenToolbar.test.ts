import { describe, expect, it } from 'vitest';
import {
  HIDE_DELAY_MS,
  HOT_ZONE_PX,
  heldOpen,
  holdVisible,
  readerToolbarVisible,
  toolbarVisible,
  type ToolbarHold,
} from './zenToolbar';

const none: ToolbarHold = {
  zen: true, hotZone: false, pointerOver: false,
  focusWithin: false, findOpen: false, localHold: false,
};

describe('holdVisible', () => {
  it('always holds outside zen', () => {
    expect(holdVisible({ ...none, zen: false })).toBe(true);
  });

  it('releases in zen once every hold drops', () => {
    expect(holdVisible(none)).toBe(false);
  });

  it.each([
    ['hotZone'], ['pointerOver'], ['focusWithin'], ['findOpen'], ['localHold'],
  ] as const)('%s alone holds the toolbar visible in zen', (k) => {
    expect(holdVisible({ ...none, [k]: true })).toBe(true);
  });
});

describe('toolbarVisible', () => {
  it('shows until the idle timer expires', () => {
    expect(toolbarVisible(none, false)).toBe(true);
    expect(toolbarVisible(none, true)).toBe(false);
  });

  it('an expired timer never hides a held toolbar', () => {
    expect(toolbarVisible({ ...none, findOpen: true }, true)).toBe(true);
    expect(toolbarVisible({ ...none, zen: false }, true)).toBe(true);
  });
});

describe('heldOpen', () => {
  it('is false outside zen with no interaction', () => {
    expect(heldOpen({ ...none, zen: false })).toBe(false);
  });

  it.each([['hotZone'], ['pointerOver'], ['focusWithin'], ['findOpen'], ['localHold']] as const)(
    '%s alone counts as held outside zen',
    (k) => {
      expect(heldOpen({ ...none, zen: false, [k]: true })).toBe(true);
    },
  );
});

describe('readerToolbarVisible', () => {
  it('hides on scroll outside zen, where nothing else would', () => {
    expect(readerToolbarVisible({ ...none, zen: false }, false, true)).toBe(false);
    expect(readerToolbarVisible({ ...none, zen: false }, false, false)).toBe(true);
  });

  it.each([['hotZone'], ['pointerOver'], ['focusWithin'], ['findOpen'], ['localHold']] as const)(
    '%s outranks scroll-hide',
    (k) => {
      expect(readerToolbarVisible({ ...none, zen: false, [k]: true }, false, true)).toBe(true);
    },
  );

  it('stays hidden in zen once the idle timer expires, scrolled or not', () => {
    expect(readerToolbarVisible(none, true, false)).toBe(false);
  });
});

it('exports sane constants', () => {
  expect(HIDE_DELAY_MS).toBeGreaterThan(0);
  expect(HOT_ZONE_PX).toBeGreaterThan(0);
});
