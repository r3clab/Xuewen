import { beforeEach, describe, expect, it } from 'vitest';
import {
  columnWidths,
  commitColumnWidth,
  commitColumnWidths,
  initColumnWidths,
  resetColumnWidths,
  setColumnWidth,
} from './columnWidths.svelte';
import { PINNED_COLUMNS, PINNED_KEYS } from './columns';

const KEY = 'xuewen-library-columns';
const saved = () => JSON.parse(localStorage.getItem(KEY) ?? 'null');

beforeEach(() => {
  resetColumnWidths(); // back to defaults (also writes storage…)
  localStorage.clear(); // …which this wipes, so each test starts clean
});

describe('columnWidths', () => {
  it('starts at the defaults from PINNED_COLUMNS', () => {
    for (const k of PINNED_KEYS) expect(columnWidths[k]).toBe(PINNED_COLUMNS[k].defaultWidth);
  });

  it('setColumnWidth clamps to the column range and does not persist', () => {
    setColumnWidth('title', 10_000);
    expect(columnWidths.title).toBe(PINNED_COLUMNS.title.maxWidth);
    setColumnWidth('title', 1);
    expect(columnWidths.title).toBe(PINNED_COLUMNS.title.minWidth);
    expect(localStorage.getItem(KEY)).toBeNull();
  });

  it('commitColumnWidth persists and initColumnWidths restores', () => {
    commitColumnWidth('venue', 200);
    expect(saved().venue).toBe(200);
    columnWidths.venue = PINNED_COLUMNS.venue.defaultWidth;
    initColumnWidths();
    expect(columnWidths.venue).toBe(200);
  });

  it('commitColumnWidths batches several columns into one record', () => {
    commitColumnWidths({ title: 400, year: 80 });
    expect(columnWidths.title).toBe(400);
    expect(columnWidths.year).toBe(80);
    expect(saved().title).toBe(400);
    expect(saved().year).toBe(80);
  });

  it('resetColumnWidths restores and persists the defaults', () => {
    commitColumnWidth('title', 400);
    resetColumnWidths();
    expect(columnWidths.title).toBe(PINNED_COLUMNS.title.defaultWidth);
    expect(saved().title).toBe(PINNED_COLUMNS.title.defaultWidth);
  });

  it('initColumnWidths tolerates corrupted storage', () => {
    localStorage.setItem(KEY, '{nope');
    initColumnWidths();
    for (const k of PINNED_KEYS) expect(columnWidths[k]).toBe(PINNED_COLUMNS[k].defaultWidth);
  });

  it('reconciles per key: unknown keys ignored, out-of-range values keep defaults', () => {
    // `authors` is the pre-split column a stale record might still carry;
    // title: 5 is below its minWidth.
    localStorage.setItem(KEY, JSON.stringify({ v: 1, authors: 160, title: 5, venue: 200 }));
    initColumnWidths();
    expect(columnWidths.title).toBe(PINNED_COLUMNS.title.defaultWidth);
    expect(columnWidths.venue).toBe(200);
  });

  it('ignores a record from a different schema version', () => {
    localStorage.setItem(KEY, JSON.stringify({ v: 2, venue: 200 }));
    initColumnWidths();
    expect(columnWidths.venue).toBe(PINNED_COLUMNS.venue.defaultWidth);
  });
});
