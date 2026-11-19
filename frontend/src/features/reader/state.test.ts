import { beforeEach, describe, expect, it } from 'vitest';
import {
  dropReaderState,
  findFocus,
  openFind,
  panelWidth,
  reader,
  setFind,
  setPanelView,
  toggleAnnotationsPanel,
  toggleSidebar,
} from './state.svelte';

beforeEach(() => {
  for (const k of Object.keys(reader.find)) delete reader.find[k];
  reader.panel = null;
  reader.lastPanel = 'thumbs';
});

describe('setFind', () => {
  it('toggles when called without an explicit state', () => {
    setFind('a');
    expect(reader.find['a']).toBe(true);
    setFind('a');
    expect(reader.find['a']).toBe(false);
  });

  it('forces the given state', () => {
    setFind('a', true);
    setFind('a', true);
    expect(reader.find['a']).toBe(true);
    setFind('a', false);
    expect(reader.find['a']).toBe(false);
  });
});

describe('toggleSidebar / setPanelView (global)', () => {
  it('opens at thumbnails first, closes on re-toggle', () => {
    toggleSidebar();
    expect(reader.panel).toBe('thumbs');
    toggleSidebar();
    expect(reader.panel).toBe(null);
  });

  it('reopens at the last-used view', () => {
    toggleSidebar();
    setPanelView('outline');
    toggleSidebar(); // close
    expect(reader.panel).toBe(null);
    toggleSidebar(); // reopen
    expect(reader.panel).toBe('outline');
  });

  it('is a single global setting, not keyed per document', () => {
    toggleSidebar();
    setPanelView('outline');
    // One shared value applies to every open paper.
    expect(reader.panel).toBe('outline');
  });
});

describe('toggleAnnotationsPanel', () => {
  it('opens straight onto annotations from closed', () => {
    toggleAnnotationsPanel();
    expect(reader.panel).toBe('annotations');
  });

  it('switches to annotations when another view is open', () => {
    setPanelView('outline');
    toggleAnnotationsPanel();
    expect(reader.panel).toBe('annotations');
  });

  it('closes only when annotations is already the open view', () => {
    toggleAnnotationsPanel();
    toggleAnnotationsPanel();
    expect(reader.panel).toBe(null);
    // and the sidebar button reopens there, not back at thumbnails
    toggleSidebar();
    expect(reader.panel).toBe('annotations');
  });
});

describe('panelWidth', () => {
  it('gives annotations more room than page numbers and headings', () => {
    expect(panelWidth('thumbs')).toBe(panelWidth('outline'));
    expect(panelWidth('annotations')).toBeGreaterThan(panelWidth('thumbs'));
  });
});

describe('openFind', () => {
  it('opens the bar and focuses that document’s find input, query selected', () => {
    const other = document.createElement('input');
    const input = document.createElement('input');
    input.value = 'previous';
    document.body.append(other, input);
    const detachOther = findFocus('b').attach(other);
    const detach = findFocus('a').attach(input);
    openFind('a');
    expect(reader.find['a']).toBe(true);
    expect(document.activeElement).toBe(input);
    expect([input.selectionStart, input.selectionEnd]).toEqual([0, 'previous'.length]);
    detach();
    detachOther();
    input.remove();
    other.remove();
  });
});

describe('dropReaderState', () => {
  it('forgets a closed document’s find state but leaves the global panel', () => {
    setFind('a', true);
    toggleSidebar();
    setPanelView('outline');
    dropReaderState('a');
    expect(reader.find['a']).toBeUndefined();
    expect(reader.panel).toBe('outline'); // global — unaffected by closing a tab
  });
});
