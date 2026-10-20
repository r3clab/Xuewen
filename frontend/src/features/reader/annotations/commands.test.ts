import { beforeEach, describe, expect, it, vi } from 'vitest';
import {
  ANNOTATION_HISTORY_TOPIC,
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

function mark(id: string, pageIndex = 0): { object: { id: string; pageIndex: number } } {
  return { object: { id, pageIndex } };
}

/// The two plugin scopes, faked. `throws` reproduces what both plugins really
/// do when asked about a document they have not seen yet — the case the
/// commands have to survive rather than propagate.
function harness() {
  let selection: ReturnType<typeof mark>[] = [];
  let topics: Record<string, { canUndo: boolean; canRedo: boolean } | undefined> = {};
  let throws = false;
  let activeId: string | null = 'p1';

  function guard(): void {
    if (throws) throw new Error('Annotation state not found for document: p1');
  }

  const marks: MarkScope = {
    getSelectedAnnotations: vi.fn(() => {
      guard();
      return selection;
    }),
    deleteAnnotations: vi.fn(() => guard()),
    deselectAnnotation: vi.fn(() => guard()),
  };
  const history: HistoryScopeLike = {
    undo: vi.fn(() => guard()),
    redo: vi.fn(() => guard()),
    getHistoryState: vi.fn(() => {
      guard();
      return { topics };
    }),
  };

  const forMarks = vi.fn(() => marks);
  const forHistory = vi.fn(() => history);
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
    forHistory,
    select: (...ids: string[]) => (selection = ids.map((id) => mark(id))),
    stack: (flags: { canUndo?: boolean; canRedo?: boolean }) =>
      (topics = {
        [ANNOTATION_HISTORY_TOPIC]: {
          canUndo: false,
          canRedo: false,
          ...flags,
        },
      }),
    otherTopicStack: () => (topics = { redaction: { canUndo: true, canRedo: true } }),
    unknownDocument: () => (throws = true),
    activeTab: (id: string | null) => (activeId = id),
  };
}

describe('deleting the selected mark', () => {
  it('deletes every selected mark in one call, and says it acted', () => {
    const h = harness();
    h.select('a1', 'a2');
    expect(h.commands.deleteSelection()).toBe(true);
    // One batched call, not one per mark: a multi-selection is deleted whole.
    expect(h.marks.deleteAnnotations).toHaveBeenCalledWith([
      { pageIndex: 0, id: 'a1' },
      { pageIndex: 0, id: 'a2' },
    ]);
  });

  it('does nothing with no mark selected', () => {
    const h = harness();
    expect(h.commands.hasSelection()).toBe(false);
    expect(h.commands.deleteSelection()).toBe(false);
    expect(h.marks.deleteAnnotations).not.toHaveBeenCalled();
  });

  it('does nothing on the library view, where there is no document at all', () => {
    const h = harness();
    h.select('a1');
    h.activeTab(null);
    expect(h.commands.hasSelection()).toBe(false);
    expect(h.commands.deleteSelection()).toBe(false);
    expect(h.marks.deleteAnnotations).not.toHaveBeenCalled();
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
});

describe('clearing the selection', () => {
  it('deselects when something is selected', () => {
    const h = harness();
    h.select('a1');
    expect(h.commands.clearSelection()).toBe(true);
    expect(h.marks.deselectAnnotation).toHaveBeenCalled();
  });

  it('reports it did nothing when nothing is selected, so Esc falls through', () => {
    const h = harness();
    expect(h.commands.clearSelection()).toBe(false);
    expect(h.marks.deselectAnnotation).not.toHaveBeenCalled();
  });
});

describe('undo and redo', () => {
  it('drives the annotations topic only, never the global timeline', () => {
    const h = harness();
    h.stack({ canUndo: true, canRedo: true });
    expect(h.commands.undo()).toBe(true);
    expect(h.commands.redo()).toBe(true);
    // An argument-less undo would take back the last action of any kind.
    expect(h.history.undo).toHaveBeenCalledWith(ANNOTATION_HISTORY_TOPIC);
    expect(h.history.redo).toHaveBeenCalledWith(ANNOTATION_HISTORY_TOPIC);
  });

  it('stands aside on an empty stack', () => {
    const h = harness();
    h.stack({ canUndo: false, canRedo: false });
    expect(h.commands.undo()).toBe(false);
    expect(h.commands.redo()).toBe(false);
    expect(h.history.undo).not.toHaveBeenCalled();
    expect(h.history.redo).not.toHaveBeenCalled();
  });

  it('ignores a stack belonging to some other topic', () => {
    const h = harness();
    h.otherTopicStack();
    expect(h.commands.undo()).toBe(false);
    expect(h.history.undo).not.toHaveBeenCalled();
  });

  it('does nothing on the library view', () => {
    const h = harness();
    h.stack({ canUndo: true });
    h.activeTab(null);
    expect(h.commands.undo()).toBe(false);
  });
});

describe('a tab whose document the plugins have not seen yet', () => {
  // Both plugins throw for an unknown document id, and the active tab is in
  // that state between opening and loading. The keystroke must be a no-op —
  // an exception here would abort the rest of handleKeydown.
  it('answers false everywhere instead of throwing', () => {
    const h = harness();
    h.select('a1');
    h.stack({ canUndo: true, canRedo: true });
    h.unknownDocument();
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

  // PdfDeck is remounted during startup (see documentsToAdopt in pdf/deck.ts).
  // If the replacement ever registers before the old instance tears down, the
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
