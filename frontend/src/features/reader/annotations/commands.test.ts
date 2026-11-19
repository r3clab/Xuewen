import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  annotationSelectionActive,
  clearAnnotationSelection,
  createAnnotationCommands,
  deleteSelectedAnnotations,
  redoAnnotation,
  registerAnnotationCommands,
  undoAnnotation,
  unregisterAnnotationCommands,
  type AnnotationCommands,
  type HistoryScopeLike,
  type MarkScope,
} from './commands';

/// The two capabilities, faked. `notReady` makes them null, as the kernel
/// answers for a document still opening; `refuses` makes every call throw, as
/// a capability does once its document has closed under it.
function harness() {
  let selected: string[] = [];
  let canUndo = false;
  let canRedo = false;
  let notReady = false;
  let refuses = false;
  let activeId: string | null = 'p1';

  function guard(): void {
    if (refuses) throw new Error('not-ready');
  }

  const marks: MarkScope = {
    selection: {
      getSelected: vi.fn(() => {
        guard();
        return selected;
      }),
      clear: vi.fn(() => guard()),
      delete: vi.fn(async () => guard()),
    },
  };
  const history: HistoryScopeLike = {
    undo: vi.fn(() => guard()),
    redo: vi.fn(() => guard()),
    canUndo: vi.fn(() => {
      guard();
      return canUndo;
    }),
    canRedo: vi.fn(() => {
      guard();
      return canRedo;
    }),
  };

  const forMarks = vi.fn(() => (notReady ? null : marks));
  const forHistory = vi.fn(() => (notReady ? null : history));
  const commands = createAnnotationCommands({
    marks: forMarks,
    history: forHistory,
    activeDocumentId: () => activeId,
  });

  return {
    commands,
    marks,
    history,
    forMarks,
    select: (...ids: string[]) => (selected = ids),
    stack: (flags: { canUndo?: boolean; canRedo?: boolean }) => {
      canUndo = flags.canUndo ?? false;
      canRedo = flags.canRedo ?? false;
    },
    notReady: () => (notReady = true),
    refuses: () => (refuses = true),
    activeTab: (id: string | null) => (activeId = id),
  };
}

describe('deleting the selected mark', () => {
  it('deletes the whole selection in one call, and says it acted', () => {
    const h = harness();
    h.select('a1', 'a2');
    expect(h.commands.deleteSelection()).toBe(true);
    expect(h.marks.selection.delete).toHaveBeenCalledTimes(1);
  });

  it('does nothing with no mark selected', () => {
    const h = harness();
    expect(h.commands.hasSelection()).toBe(false);
    expect(h.commands.deleteSelection()).toBe(false);
    expect(h.marks.selection.delete).not.toHaveBeenCalled();
  });

  it('does nothing on the library view, where there is no document at all', () => {
    const h = harness();
    h.select('a1');
    h.activeTab(null);
    expect(h.commands.hasSelection()).toBe(false);
    expect(h.commands.deleteSelection()).toBe(false);
    expect(h.marks.selection.delete).not.toHaveBeenCalled();
  });

  it('acts on the tab the keystroke belongs to, read at call time', () => {
    const h = harness();
    h.select('a1');
    h.commands.deleteSelection();
    h.activeTab('p2');
    h.commands.deleteSelection();
    // The active tab is read per call, never captured when the commands were
    // built — PdfDeck registers them once and the reader switches tabs for the
    // rest of the session.
    expect(h.forMarks).toHaveBeenNthCalledWith(1, 'p1');
    expect(h.forMarks).toHaveBeenNthCalledWith(2, 'p2');
  });

  it('swallows a refused delete', async () => {
    const h = harness();
    h.select('a1');
    vi.mocked(h.marks.selection.delete).mockRejectedValueOnce(new Error('permission-denied'));
    expect(h.commands.deleteSelection()).toBe(true);
    await Promise.resolve();
  });
});

describe('clearing the selection', () => {
  it('deselects when something is selected', () => {
    const h = harness();
    h.select('a1');
    expect(h.commands.clearSelection()).toBe(true);
    expect(h.marks.selection.clear).toHaveBeenCalled();
  });

  it('reports it did nothing when nothing is selected, so Esc falls through', () => {
    const h = harness();
    expect(h.commands.clearSelection()).toBe(false);
    expect(h.marks.selection.clear).not.toHaveBeenCalled();
  });
});

describe('undo and redo', () => {
  it('drives the document history', () => {
    const h = harness();
    h.stack({ canUndo: true, canRedo: true });
    expect(h.commands.undo()).toBe(true);
    expect(h.commands.redo()).toBe(true);
    expect(h.history.undo).toHaveBeenCalledTimes(1);
    expect(h.history.redo).toHaveBeenCalledTimes(1);
  });

  it('stands aside on an empty stack', () => {
    const h = harness();
    h.stack({ canUndo: false, canRedo: false });
    expect(h.commands.undo()).toBe(false);
    expect(h.commands.redo()).toBe(false);
    expect(h.history.undo).not.toHaveBeenCalled();
    expect(h.history.redo).not.toHaveBeenCalled();
  });

  it('does nothing on the library view', () => {
    const h = harness();
    h.stack({ canUndo: true });
    h.activeTab(null);
    expect(h.commands.undo()).toBe(false);
  });
});

describe('a tab whose document is not ready', () => {
  it('answers false everywhere while the capabilities are not there yet', () => {
    const h = harness();
    h.select('a1');
    h.stack({ canUndo: true, canRedo: true });
    h.notReady();
    expect(h.commands.hasSelection()).toBe(false);
    expect(h.commands.deleteSelection()).toBe(false);
    expect(h.commands.clearSelection()).toBe(false);
    expect(h.commands.undo()).toBe(false);
    expect(h.commands.redo()).toBe(false);
  });

  // An exception here would abort the rest of handleKeydown.
  it('answers false everywhere instead of throwing once they refuse', () => {
    const h = harness();
    h.select('a1');
    h.stack({ canUndo: true, canRedo: true });
    h.refuses();
    expect(() => h.commands.hasSelection()).not.toThrow();
    expect(h.commands.hasSelection()).toBe(false);
    expect(h.commands.deleteSelection()).toBe(false);
    expect(h.commands.clearSelection()).toBe(false);
    expect(h.commands.undo()).toBe(false);
    expect(h.commands.redo()).toBe(false);
  });
});

describe('the module-level registration', () => {
  const calls: string[] = [];
  const spy: AnnotationCommands = {
    hasSelection: () => true,
    deleteSelection: () => (calls.push('delete'), true),
    clearSelection: () => (calls.push('clear'), true),
    undo: () => (calls.push('undo'), true),
    redo: () => (calls.push('redo'), true),
  };

  beforeEach(() => {
    calls.length = 0;
    registerAnnotationCommands(null);
  });

  it('is inert with no reader mounted — the library view, and every other test file', () => {
    expect(annotationSelectionActive()).toBe(false);
    expect(deleteSelectedAnnotations()).toBe(false);
    expect(clearAnnotationSelection()).toBe(false);
    expect(undoAnnotation()).toBe(false);
    expect(redoAnnotation()).toBe(false);
  });

  it('forwards to the registered commands, and stops when they are cleared', () => {
    registerAnnotationCommands(spy);
    expect(annotationSelectionActive()).toBe(true);
    deleteSelectedAnnotations();
    clearAnnotationSelection();
    undoAnnotation();
    redoAnnotation();
    expect(calls).toEqual(['delete', 'clear', 'undo', 'redo']);
    registerAnnotationCommands(null);
    expect(deleteSelectedAnnotations()).toBe(false);
    expect(calls).toHaveLength(4);
  });

  // If a replacement ever registers before the old instance tears down, the
  // old one's cleanup must not take the live commands with it.
  it('unregisters by identity, so a replacement survives the old teardown', () => {
    const replacement: AnnotationCommands = {
      ...spy,
      deleteSelection: () => (calls.push('new'), true),
    };
    registerAnnotationCommands(spy);
    registerAnnotationCommands(replacement);
    unregisterAnnotationCommands(spy); // the outgoing instance's cleanup
    expect(deleteSelectedAnnotations()).toBe(true);
    expect(calls).toEqual(['new']);
    unregisterAnnotationCommands(replacement);
    expect(deleteSelectedAnnotations()).toBe(false);
  });
});
