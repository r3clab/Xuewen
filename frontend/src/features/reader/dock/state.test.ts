import { beforeEach, describe, expect, it } from 'vitest';
import { viewer } from '$features/reader/tabs/state.svelte';
import { closeDock, dock, initDock, openDock, toggleDock } from './state.svelte';

beforeEach(() => {
  localStorage.clear();
  dock.open = false;
  dock.entry = null;
  viewer.activeId = 'p1';
});

describe('dock state', () => {
  it('toggleDock opens with the requested entry point', () => {
    toggleDock('ask');
    expect(dock.open).toBe(true);
    expect(dock.entry).toBe('ask');
  });

  it('i closes an open dock; c re-requests the composer instead', () => {
    openDock('record');
    toggleDock('ask'); // asks for the composer — stays open
    expect(dock.open).toBe(true);
    expect(dock.entry).toBe('ask');
    dock.entry = null; // consumed by ReaderDock
    toggleDock('ask');
    expect(dock.open).toBe(true);
    expect(dock.entry).toBe('ask');
    toggleDock('record');
    expect(dock.open).toBe(false);
  });

  it('toggleDock is a no-op without an active PDF tab', () => {
    viewer.activeId = null;
    toggleDock('record');
    expect(dock.open).toBe(false);
  });

  it('open/close persist and initDock restores them', () => {
    openDock('ask');
    dock.open = false;
    initDock();
    expect(dock.open).toBe(true);
    closeDock();
    dock.open = true;
    initDock();
    expect(dock.open).toBe(false);
  });

  it('the entry point is a one-shot request, never persisted', () => {
    openDock('ask');
    dock.entry = null;
    initDock();
    expect(dock.entry).toBeNull();
    closeDock();
    expect(dock.entry).toBeNull();
  });

  it('initDock tolerates corrupted storage', () => {
    localStorage.setItem('xuewen-dock', '{nope');
    initDock();
    expect(dock.open).toBe(false);
  });
});
