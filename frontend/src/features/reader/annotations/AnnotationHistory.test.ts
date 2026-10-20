import { tick } from 'svelte';
import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createAnnotationCommands, registerAnnotationCommands } from './commands';

type Listener = (topic: string | undefined) => void;
type Flags = { canUndo: boolean; canRedo: boolean };

const listeners = new Set<Listener>();
let topics: Record<string, Flags> = {};

/// The plugin's per-document scope, backed by a stack the tests drive. Only
/// `topics` matters here: the component reads its own topic, never `global`.
const scope = {
  undo: vi.fn(),
  redo: vi.fn(),
  getHistoryState: () => ({ global: { canUndo: false, canRedo: false }, topics }),
  onHistoryChange: (l: Listener) => {
    listeners.add(l);
    return () => listeners.delete(l);
  },
};
const forDocument = vi.fn(() => scope);

vi.mock('@embedpdf/plugin-history/svelte', () => ({
  useHistoryCapability: () => ({ provides: { forDocument }, isLoading: false }),
}));

import AnnotationHistory from './AnnotationHistory.svelte';

/// What the plugin does after every registered command, undo, or redo.
async function historyChanged(flags: Partial<Flags>): Promise<void> {
  topics = { annotations: { canUndo: false, canRedo: false, ...flags } };
  for (const l of listeners) l('annotations');
  await tick();
}

beforeEach(() => {
  vi.clearAllMocks();
  listeners.clear();
  topics = {};
  // The buttons act through the shared commands, the same path ⌘Z takes, so
  // the test has to stand in for the registration PdfDeck does in the app.
  registerAnnotationCommands(
    createAnnotationCommands({
      marks: () => ({
        getSelectedAnnotations: () => [],
        deleteAnnotations: () => {},
        deselectAnnotation: () => {},
      }),
      history: () => scope,
      activeDocumentId: () => 'p1',
    }),
  );
});

afterEach(() => registerAnnotationCommands(null));

const props = { documentId: 'p1' };
const undoBtn = () => screen.getByRole('button', { name: /undo annotation/i });
const redoBtn = () => screen.getByRole('button', { name: /redo annotation/i });

describe('the annotation undo/redo buttons', () => {
  it('start disabled on a paper nobody has annotated yet', () => {
    render(AnnotationHistory, { props });
    expect(undoBtn()).toBeDisabled();
    expect(redoBtn()).toBeDisabled();
  });

  it('reads the stack a document already has, not just later changes', () => {
    topics = { annotations: { canUndo: true, canRedo: false } };
    render(AnnotationHistory, { props });
    expect(undoBtn()).toBeEnabled();
  });

  it('follows the plugin as marks are drawn and taken back', async () => {
    render(AnnotationHistory, { props });
    await historyChanged({ canUndo: true });
    expect(undoBtn()).toBeEnabled();
    expect(redoBtn()).toBeDisabled();
    await historyChanged({ canRedo: true });
    expect(undoBtn()).toBeDisabled();
    expect(redoBtn()).toBeEnabled();
  });

  it('undoes and redoes only annotations, never the global timeline', async () => {
    render(AnnotationHistory, { props });
    await historyChanged({ canUndo: true, canRedo: true });
    await userEvent.click(undoBtn());
    await userEvent.click(redoBtn());
    // Passing the topic is what keeps undo off whatever else may later share
    // the history plugin; an argument-less undo would take back the last
    // action of any kind. Reaching the plugin at all also pins the click to
    // the shared commands, so the button and ⌘Z cannot diverge.
    expect(scope.undo).toHaveBeenCalledWith('annotations');
    expect(scope.redo).toHaveBeenCalledWith('annotations');
  });

  it('scopes to its own document, so one tab’s undo cannot reach another', () => {
    render(AnnotationHistory, { props: { documentId: 'p2' } });
    expect(forDocument).toHaveBeenCalledWith('p2');
  });

  it('stops listening when the tab closes', () => {
    const { unmount } = render(AnnotationHistory, { props });
    expect(listeners.size).toBe(1);
    unmount();
    expect(listeners.size).toBe(0);
  });
});
