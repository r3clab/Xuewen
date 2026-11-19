import { beforeEach, describe, expect, it } from 'vitest';
import { viewer } from '$features/reader/tabs/state.svelte';
import { askInDock, closeDock, composerFocus, dock, initDock, openDock, toggleDock } from './state.svelte';

beforeEach(() => {
  localStorage.clear();
  dock.open = false;
  viewer.activeId = 'p1';
});

describe('dock state', () => {
  it('i toggles the dock', () => {
    toggleDock();
    expect(dock.open).toBe(true);
    toggleDock();
    expect(dock.open).toBe(false);
  });

  it('c opens the dock and focuses the composer, and never closes it', () => {
    const composer = document.createElement('textarea');
    document.body.append(composer);
    const detach = composerFocus.attach(composer);
    askInDock();
    expect(dock.open).toBe(true);
    expect(document.activeElement).toBe(composer);
    composer.blur();
    askInDock();
    expect(dock.open).toBe(true);
    expect(document.activeElement).toBe(composer);
    detach();
    composer.remove();
  });

  it('both are no-ops without an active PDF tab', () => {
    viewer.activeId = null;
    toggleDock();
    askInDock();
    expect(dock.open).toBe(false);
  });

  it('open/close persist and initDock restores them', () => {
    openDock();
    dock.open = false;
    initDock();
    expect(dock.open).toBe(true);
    closeDock();
    dock.open = true;
    initDock();
    expect(dock.open).toBe(false);
  });

  it('initDock tolerates corrupted storage', () => {
    localStorage.setItem('xuewen-dock', '{nope');
    initDock();
    expect(dock.open).toBe(false);
  });
});
