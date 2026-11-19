import { describe, expect, it } from 'vitest';
import { planOpens, reconcileDocuments } from './deck';

describe('reconcileDocuments', () => {
  it('opens new tabs and closes removed ones', () => {
    const { toOpen, toClose } = reconcileDocuments(['a', 'b'], ['b', 'c']);
    expect(toOpen).toEqual(['c']);
    expect(toClose).toEqual(['a']);
  });

  it('is a no-op when opened matches the tabs exactly', () => {
    const { toOpen, toClose } = reconcileDocuments(['a', 'b'], ['a', 'b']);
    expect(toOpen).toEqual([]);
    expect(toClose).toEqual([]);
  });

  it('opens everything when nothing is opened yet', () => {
    const { toOpen, toClose } = reconcileDocuments([], ['a', 'b', 'c']);
    expect(toOpen).toEqual(['a', 'b', 'c']);
    expect(toClose).toEqual([]);
  });

  it('closes everything when there are no tabs left', () => {
    const { toOpen, toClose } = reconcileDocuments(['a', 'b'], []);
    expect(toOpen).toEqual([]);
    expect(toClose).toEqual(['a', 'b']);
  });
});

describe('planOpens', () => {
  it('opens the tab on screen first and defers the rest', () => {
    // The restored-session case: four tabs, one of them visible.
    const { now, deferred } = planOpens(['a', 'b', 'c', 'd'], 'c');
    expect(now).toEqual(['c']);
    expect(deferred).toEqual(['a', 'b', 'd']);
  });

  it('keeps the deferred tabs in tab order', () => {
    const { deferred } = planOpens(['a', 'b', 'c'], 'a');
    expect(deferred).toEqual(['b', 'c']);
  });

  it('defers everything when the active document is already open', () => {
    // `toOpen` excludes it, so there is nothing on screen left to prioritise —
    // the queued tabs must still drain rather than stall forever.
    const { now, deferred } = planOpens(['b', 'c'], 'a');
    expect(now).toEqual([]);
    expect(deferred).toEqual(['b', 'c']);
  });

  it('defers everything when no tab is active', () => {
    const { now, deferred } = planOpens(['a', 'b'], null);
    expect(now).toEqual([]);
    expect(deferred).toEqual(['a', 'b']);
  });

  it('opening a single paper is unaffected', () => {
    // The common case — one click, one document, opened immediately.
    const { now, deferred } = planOpens(['a'], 'a');
    expect(now).toEqual(['a']);
    expect(deferred).toEqual([]);
  });

  it('never lists a document in both halves', () => {
    const { now, deferred } = planOpens(['a', 'b', 'c'], 'b');
    expect(now.filter((id) => deferred.includes(id))).toEqual([]);
    expect([...now, ...deferred].sort()).toEqual(['a', 'b', 'c']);
  });
});

