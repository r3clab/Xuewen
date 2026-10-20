import { describe, expect, it } from 'vitest';
import {
  ICON_COLUMN_PX,
  PINNED_COLUMNS,
  PINNED_KEYS,
  TAGS_MIN_PX,
  TAGS_TARGET_PX,
  autoFitBudget,
  chromePx,
  dragCeiling,
  tableMinWidth,
  type PinnedColumnKey,
} from './columns';

describe('PINNED_COLUMNS', () => {
  it('keeps min <= default <= max and a label for every column', () => {
    for (const key of PINNED_KEYS) {
      const def = PINNED_COLUMNS[key];
      expect(def.minWidth).toBeLessThanOrEqual(def.defaultWidth);
      expect(def.defaultWidth).toBeLessThanOrEqual(def.maxWidth);
      expect(def.label.length).toBeGreaterThan(0);
    }
  });
});

function defaults(): Record<PinnedColumnKey, number> {
  return Object.fromEntries(PINNED_KEYS.map((k) => [k, PINNED_COLUMNS[k].defaultWidth])) as Record<
    PinnedColumnKey,
    number
  >;
}

describe('width budget', () => {
  it('tableMinWidth is chrome + every pinned width + the Tags reserve', () => {
    const widths = defaults();
    const pinned = PINNED_KEYS.reduce((s, k) => s + widths[k], 0);
    expect(chromePx()).toBe(2 * ICON_COLUMN_PX);
    expect(tableMinWidth(widths)).toBe(chromePx() + pinned + TAGS_MIN_PX);
  });

  it('dragCeiling lets one column absorb exactly the pane surplus', () => {
    const widths = defaults();
    const others = PINNED_KEYS.filter((k) => k !== 'title').reduce((s, k) => s + widths[k], 0);
    const pane = 1200;
    expect(dragCeiling('title', widths, pane)).toBe(pane - chromePx() - others - TAGS_MIN_PX);
  });

  it('dragCeiling clamps to the static cap above and the column min below', () => {
    const widths = defaults();
    // A huge pane can't push past the sanity cap…
    expect(dragCeiling('year', widths, 10_000)).toBe(PINNED_COLUMNS.year.maxWidth);
    // …and a tiny pane can't squeeze below the column's own minimum.
    expect(dragCeiling('title', widths, 300)).toBe(PINNED_COLUMNS.title.minWidth);
  });

  it('dragCeiling falls back to the static cap without layout (jsdom)', () => {
    expect(dragCeiling('title', defaults(), 0)).toBe(PINNED_COLUMNS.title.maxWidth);
  });

  it('autoFitBudget reserves the chrome and the comfortable Tags strip', () => {
    expect(autoFitBudget(1000)).toBe(1000 - chromePx() - TAGS_TARGET_PX);
  });
});
