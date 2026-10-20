import { tick } from 'svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { handleKeydown, isEditable } from './shortcuts';
import { cancelLeader, leader } from './leader.svelte';
import { registerAnnotationCommands, type AnnotationCommands } from '$features/reader/annotations/commands';
import { chat } from '$features/reader/ask/chat.svelte';
import { registerPdfCopy } from '$features/reader/pdfCopy';
import { identifyState } from '$features/identify/state.svelte';
import { importState } from '$features/importing/queue.svelte';
import { library } from '$features/library/state.svelte';
import { selection, viewer } from '$features/reader/tabs/state.svelte';
import { shell } from '$app/shell.svelte';
import { dock } from '$features/reader/dock/state.svelte';
import { finder } from '$features/library/search/state.svelte';
import { reader } from '$features/reader/state.svelte';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';

function paper(id: string): PaperSummary {
  return {
    id, title: id, authors: [], venue: null, year: null, doi: null, arxiv_id: null,
    dblp_key: null, cite_key: null, url: null, source: null, status: 'resolved',
    added_at: '', name: null, starred: false, tags: [], projects: [],
  };
}

function key(k: string, opts: Partial<KeyboardEvent> & { target?: EventTarget } = {}): KeyboardEvent {
  const e = new KeyboardEvent('keydown', { key: k, ...opts });
  if (opts.target) Object.defineProperty(e, 'target', { value: opts.target });
  return e;
}

beforeEach(() => {
  library.papers = [paper('a'), paper('b'), paper('c')];
  viewer.tabs = [];
  viewer.activeId = null;
  dock.open = false;
  dock.entry = null;
  selection.id = null;
  viewer.zen = false;
  finder.open = false;
  cancelLeader();
  importState.open = false;
  shell.helpOpen = false;
  identifyState.open = false;
  chat.available = false;
  localStorage.clear();
  for (const k of Object.keys(reader.find)) delete reader.find[k];
  reader.panel = null;
  reader.lastPanel = 'thumbs';
});

describe('isEditable', () => {
  it('flags inputs and textareas', () => {
    expect(isEditable(document.createElement('input'))).toBe(true);
    expect(isEditable(document.createElement('textarea'))).toBe(true);
    expect(isEditable(document.createElement('div'))).toBe(false);
    expect(isEditable(null)).toBe(false);
  });
});

describe('handleKeydown', () => {
  it('Space then f opens the file picker', () => {
    handleKeydown(key(' '));
    expect(leader.pending).toEqual([' ']);
    handleKeydown(key('f'));
    expect(finder.open).toBe(true);
    expect(leader.pending).toEqual([]);
  });

  it('Space stands aside in a text field and over a focused control', () => {
    handleKeydown(key(' ', { target: document.createElement('input') }));
    expect(leader.pending).toEqual([]);
    handleKeydown(key(' ', { target: document.createElement('button') }));
    expect(leader.pending).toEqual([]);
  });

  it('an unbound second key cancels the sequence without dispatching itself', () => {
    handleKeydown(key(' '));
    handleKeydown(key('j'));
    expect(finder.open).toBe(false);
    expect(leader.pending).toEqual([]);
    // `j` would otherwise have moved the library selection.
    expect(selection.id).toBe(null);
  });

  it('every key is inert while the picker owns the screen', () => {
    finder.open = true;
    handleKeydown(key('j'));
    expect(selection.id).toBe(null);
  });

  it('j/k move the selection through the list, Enter opens it', () => {
    handleKeydown(key('j'));
    expect(selection.id).toBe('a');
    handleKeydown(key('j'));
    expect(selection.id).toBe('b');
    handleKeydown(key('k'));
    expect(selection.id).toBe('a');
    handleKeydown(key('Enter'));
    expect(viewer.activeId).toBe('a');
  });

  it('z toggles zen only with an active tab; x closes the active tab', () => {
    handleKeydown(key('z'));
    expect(viewer.zen).toBe(false);
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('z'));
    expect(viewer.zen).toBe(true);
    handleKeydown(key('x'));
    expect(viewer.tabs).toHaveLength(0);
    expect(viewer.zen).toBe(false);
  });

  it('matches letters case-insensitively (Caps Lock / Shift → uppercase)', () => {
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    expect(viewer.activeId).toBe('a');
    handleKeydown(key('Z')); // uppercase, e.g. Caps Lock on
    expect(viewer.zen).toBe(true);
    handleKeydown(key('X')); // uppercase close-tab
    expect(viewer.tabs).toHaveLength(0);
    expect(viewer.zen).toBe(false);
  });

  it('a modifier shortcut mid-chord abandons the sequence', () => {
    handleKeydown(key(' '));
    handleKeydown(key('z', { metaKey: true }));
    expect(leader.pending).toEqual([]);
    // `f` is now an ordinary key again, not the tail of the abandoned chord.
    handleKeydown(key('f'));
    expect(finder.open).toBe(false);
  });

  it('Escape abandons a pending chord first, then exits zen', () => {
    viewer.zen = true;
    handleKeydown(key(' '));
    handleKeydown(key('Escape'));
    expect(leader.pending).toEqual([]);
    expect(viewer.zen).toBe(true);
    handleKeydown(key('Escape'));
    expect(viewer.zen).toBe(false);
  });

  it('c opens the dock on the composer only with an active tab and available chat', () => {
    handleKeydown(key('c'));
    expect(dock.open).toBe(false);
    chat.available = true;
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('c'));
    expect(dock.open).toBe(true);
    expect(dock.entry).toBe('ask');
    // c asks for the composer, so a repeat re-requests focus rather than
    // closing the panel it was asked to type into.
    dock.entry = null;
    handleKeydown(key('c'));
    expect(dock.open).toBe(true);
    expect(dock.entry).toBe('ask');
  });

  it('Escape closes the dock before exiting zen', () => {
    chat.available = true;
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('z'));
    handleKeydown(key('c'));
    expect(viewer.zen).toBe(true);
    expect(dock.open).toBe(true);
    handleKeydown(key('Escape'));
    expect(dock.open).toBe(false);
    expect(viewer.zen).toBe(true);
    handleKeydown(key('Escape'));
    expect(viewer.zen).toBe(false);
  });

  it('i toggles the dock only with an active tab', () => {
    handleKeydown(key('i'));
    expect(dock.open).toBe(false); // no active paper
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('i'));
    expect(dock.open).toBe(true);
    expect(dock.entry).toBe('record');
    handleKeydown(key('i'));
    expect(dock.open).toBe(false);
  });

  it('a toggles the annotations panel only with an active tab', () => {
    handleKeydown(key('a'));
    expect(reader.panel).toBe(null); // no active paper — nowhere to show it
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('a'));
    expect(reader.panel).toBe('annotations');
    handleKeydown(key('a'));
    expect(reader.panel).toBe(null);
  });

  it('a switches an already-open panel to annotations rather than closing it', () => {
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    reader.panel = 'thumbs';
    handleKeydown(key('a'));
    expect(reader.panel).toBe('annotations');
  });

  it('Escape closes a dock opened directly (not via a shortcut) before exiting zen', () => {
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    handleKeydown(key('z'));
    dock.open = true;
    expect(viewer.zen).toBe(true);
    handleKeydown(key('Escape'));
    expect(dock.open).toBe(false);
    expect(viewer.zen).toBe(true);
    handleKeydown(key('Escape'));
    expect(viewer.zen).toBe(false);
  });

  it('/ exits zen and focuses the search input', async () => {
    const input = document.createElement('input');
    input.setAttribute('data-search-input', '');
    document.body.appendChild(input);
    viewer.zen = true;
    handleKeydown(key('/'));
    expect(viewer.zen).toBe(false);
    await tick();
    expect(document.activeElement).toBe(input);
    input.remove();
  });

  it('Enter on a focused button activates the button only, not the selection', () => {
    handleKeydown(key('j')); // selection = first paper
    const btn = document.createElement('button');
    handleKeydown(key('Enter', { target: btn }));
    expect(viewer.tabs).toHaveLength(0);
  });

  // A keydown that bubbled out of a shadow DOM (e.g. the PDF viewer): the
  // browser retargets `target` to the non-editable host, but composedPath()[0]
  // is the real element inside the shadow tree.
  function shadowKey(k: string, realTarget: EventTarget, host: EventTarget): KeyboardEvent {
    const e = new KeyboardEvent('keydown', { key: k });
    Object.defineProperty(e, 'target', { value: host });
    Object.defineProperty(e, 'composedPath', { value: () => [realTarget, host, window] });
    return e;
  }

  it('ignores shortcuts when a shadow-DOM input (viewer find box) is the real target', () => {
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    expect(viewer.activeId).toBe('a');
    const host = document.createElement('embedpdf-container'); // non-editable shadow host
    const input = document.createElement('input'); // real target inside the shadow tree
    handleKeydown(shadowKey('x', input, host));
    expect(viewer.tabs).toHaveLength(1); // NOT closed — the key belongs to the find box
  });

  it('Enter on a shadow-DOM button activates the button only, not the selection', () => {
    handleKeydown(key('j')); // selection = first paper
    const host = document.createElement('embedpdf-container');
    const btn = document.createElement('button'); // real target inside the shadow tree
    handleKeydown(shadowKey('Enter', btn, host));
    expect(viewer.tabs).toHaveLength(0); // guard sees the button, not its host
  });

  it('still fires shortcuts when the real shadow target is non-editable (viewport)', () => {
    handleKeydown(key('j'));
    handleKeydown(key('Enter'));
    const host = document.createElement('embedpdf-container');
    const viewport = document.createElement('div'); // non-editable
    handleKeydown(shadowKey('x', viewport, host));
    expect(viewer.tabs).toHaveLength(0); // closed — shortcuts still work while reading
  });

  it('single-key shortcuts are inert while a modal is open', () => {
    importState.open = true;
    handleKeydown(key('j'));
    expect(selection.id).toBe(null);
    handleKeydown(key('Escape')); // the modal owns Esc
    expect(importState.open).toBe(true); // handler must not touch it
  });

  it('? opens the shortcut help overlay', () => {
    handleKeydown(key('?', { shiftKey: true }));
    expect(shell.helpOpen).toBe(true);
  });

  it('? is inert while focus is in a text control', () => {
    handleKeydown(key('?', { target: document.createElement('input') }));
    expect(shell.helpOpen).toBe(false);
  });

  it('single-key shortcuts are inert while the help overlay is open', () => {
    shell.helpOpen = true;
    handleKeydown(key('z'));
    expect(viewer.zen).toBe(false);
  });

  it('cmd+f opens the find bar for the active paper', () => {
    viewer.tabs = [{ id: 'a', title: 'A', name: null }];
    viewer.activeId = 'a';
    const e = key('f', { metaKey: true, cancelable: true });
    handleKeydown(e);
    expect(reader.find['a']).toBe(true);
    expect(e.defaultPrevented).toBe(true); // the browser find must not fire
  });

  it('cmd+f on the Library view leaves the browser find alone', () => {
    viewer.activeId = null;
    const e = key('f', { metaKey: true, cancelable: true });
    handleKeydown(e);
    expect(e.defaultPrevented).toBe(false);
  });

  it('cmd+f is inert while a modal is open', () => {
    viewer.tabs = [{ id: 'a', title: 'A', name: null }];
    viewer.activeId = 'a';
    importState.open = true;
    handleKeydown(key('f', { metaKey: true, cancelable: true }));
    expect(reader.find['a']).toBeUndefined();
  });
});

describe('cmd+c in the reader', () => {
  let copied = 0;
  let hasSel = true;

  function openReader(): void {
    viewer.tabs = [{ id: 'a', title: 'A', name: null }];
    viewer.activeId = 'a';
  }

  beforeEach(() => {
    copied = 0;
    hasSel = true;
    registerPdfCopy({
      hasSelection: () => hasSel,
      fetchPending: () => false,
      copySelection: async () => {
        copied += 1;
      },
      forget: () => {},
      destroy: () => {},
    });
  });

  // Registered globally by PdfDeck in the app, so it must not leak into the
  // sibling test files that share this module.
  afterEach(() => registerPdfCopy(null));

  it('copies the PDF selection and swallows the browser copy', () => {
    openReader();
    const e = key('c', { metaKey: true, cancelable: true });
    handleKeydown(e);
    expect(copied).toBe(1);
    expect(e.defaultPrevented).toBe(true);
  });

  it('stands aside on the Library view', () => {
    viewer.activeId = null;
    const e = key('c', { metaKey: true, cancelable: true });
    handleKeydown(e);
    expect(copied).toBe(0);
    expect(e.defaultPrevented).toBe(false);
  });

  it('stands aside while focus is in a text control', () => {
    openReader();
    const e = key('c', { metaKey: true, cancelable: true, target: document.createElement('textarea') });
    handleKeydown(e);
    expect(copied).toBe(0);
    expect(e.defaultPrevented).toBe(false);
  });

  it('stands aside when the browser has its own text selection', () => {
    openReader();
    const real = document.getSelection;
    document.getSelection = () => ({ toString: () => 'text from the details dock' }) as Selection;
    try {
      const e = key('c', { metaKey: true, cancelable: true });
      handleKeydown(e);
      expect(copied).toBe(0);
      expect(e.defaultPrevented).toBe(false);
    } finally {
      document.getSelection = real;
    }
  });

  it('stands aside when nothing is selected in the PDF', () => {
    openReader();
    hasSel = false;
    const e = key('c', { metaKey: true, cancelable: true });
    handleKeydown(e);
    expect(copied).toBe(0);
    expect(e.defaultPrevented).toBe(false);
  });

  it('leaves cmd+alt+c and cmd+shift+c to the browser', () => {
    openReader();
    for (const mods of [{ altKey: true }, { shiftKey: true }]) {
      const e = key('c', { metaKey: true, cancelable: true, ...mods });
      handleKeydown(e);
      expect(e.defaultPrevented).toBe(false);
    }
    expect(copied).toBe(0);
  });

  it('is inert while a modal is open', () => {
    openReader();
    shell.helpOpen = true;
    const e = key('c', { metaKey: true, cancelable: true });
    handleKeydown(e);
    expect(copied).toBe(0);
    expect(e.defaultPrevented).toBe(false);
  });

  it('leaves the bare c shortcut alone', () => {
    openReader();
    chat.available = true;
    handleKeydown(key('c'));
    expect(copied).toBe(0);
    expect(dock.open).toBe(true);
    expect(dock.entry).toBe('ask');
  });
});

describe('annotation keys in the reader', () => {
  const calls: string[] = [];
  /// The commands PdfDeck registers in the app, faked. `acted` is what a real
  /// command reports when there is nothing selected or the stack is empty —
  /// the signal shortcuts.ts uses to leave the keystroke to the browser.
  let acted = true;

  function record(name: string): boolean {
    calls.push(name);
    return acted;
  }

  const commands: AnnotationCommands = {
    hasSelection: () => acted,
    deleteSelection: () => record('delete'),
    clearSelection: () => record('clear'),
    undo: () => record('undo'),
    redo: () => record('redo'),
  };

  function openReader(): void {
    viewer.tabs = [{ id: 'a', title: 'A', name: null }];
    viewer.activeId = 'a';
  }

  beforeEach(() => {
    calls.length = 0;
    acted = true;
    registerAnnotationCommands(commands);
  });

  // Registered globally by PdfDeck in the app, so it must not leak into the
  // sibling test files that share this module.
  afterEach(() => registerAnnotationCommands(null));

  it('Delete and Backspace remove the selected mark, and swallow the key', () => {
    openReader();
    for (const k of ['Delete', 'Backspace']) {
      const e = key(k, { cancelable: true });
      handleKeydown(e);
      expect(e.defaultPrevented).toBe(true); // Backspace must not navigate back
    }
    expect(calls).toEqual(['delete', 'delete']);
  });

  it('Delete and Backspace are inert with nothing selected', () => {
    openReader();
    acted = false;
    const e = key('Backspace', { cancelable: true });
    handleKeydown(e);
    expect(calls).toEqual([]);
    expect(e.defaultPrevented).toBe(false);
  });

  it('Delete is inert while typing', () => {
    openReader();
    handleKeydown(key('Delete', { target: document.createElement('input') }));
    expect(calls).toEqual([]);
  });

  it('cmd+z undoes and shift+cmd+z redoes, swallowing the key', () => {
    openReader();
    const undo = key('z', { metaKey: true, cancelable: true });
    handleKeydown(undo);
    const redo = key('z', { metaKey: true, shiftKey: true, cancelable: true });
    handleKeydown(redo);
    expect(calls).toEqual(['undo', 'redo']);
    expect(undo.defaultPrevented).toBe(true);
    expect(redo.defaultPrevented).toBe(true);
  });

  it('cmd+z leaves the keystroke to the browser when the stack is empty', () => {
    openReader();
    acted = false;
    const e = key('z', { metaKey: true, cancelable: true });
    handleKeydown(e);
    expect(calls).toEqual(['undo']);
    expect(e.defaultPrevented).toBe(false);
  });

  it('cmd+z stands aside in a text control, where the browser owns undo', () => {
    openReader();
    const e = key('z', { metaKey: true, cancelable: true, target: document.createElement('textarea') });
    handleKeydown(e);
    expect(calls).toEqual([]);
    expect(e.defaultPrevented).toBe(false);
  });

  it('cmd+z is inert while a modal is open', () => {
    openReader();
    importState.open = true;
    handleKeydown(key('z', { metaKey: true, cancelable: true }));
    expect(calls).toEqual([]);
  });

  it('leaves the bare z shortcut (zen) alone', () => {
    openReader();
    handleKeydown(key('z'));
    expect(calls).toEqual([]);
    expect(viewer.zen).toBe(true);
  });

  it('Escape deselects the mark before closing the dock or leaving zen', () => {
    openReader();
    viewer.zen = true;
    dock.open = true;
    handleKeydown(key('Escape'));
    expect(calls).toEqual(['clear']);
    expect(dock.open).toBe(true);
    expect(viewer.zen).toBe(true);
    // With the selection gone, Esc resumes its usual cascade.
    acted = false;
    handleKeydown(key('Escape'));
    expect(dock.open).toBe(false);
    expect(viewer.zen).toBe(true);
  });
});
