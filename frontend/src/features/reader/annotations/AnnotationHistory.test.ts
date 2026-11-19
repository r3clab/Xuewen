import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createAnnotationCommands, registerAnnotationCommands } from './commands';

const state = vi.hoisted(() => ({ canUndo: false, canRedo: false }));

vi.mock('@embedpdf/svelte/history', () => ({ useHistoryState: () => state }));

import AnnotationHistory from './AnnotationHistory.svelte';

const scope = {
  undo: vi.fn(),
  redo: vi.fn(),
  canUndo: () => state.canUndo,
  canRedo: () => state.canRedo,
};

let unregister: () => void;
beforeEach(() => {
  vi.clearAllMocks();
  state.canUndo = false;
  state.canRedo = false;
  // The buttons act through the shared commands, the same path ⌘Z takes, so
  // the test has to stand in for the registration PdfDeck does in the app.
  unregister = registerAnnotationCommands(
    createAnnotationCommands({
      marks: () => ({ selection: { getSelected: () => [], clear: () => {}, delete: async () => {} } }),
      history: () => scope,
      activeDocumentId: () => 'p1',
    }),
  );
});

afterEach(() => unregister());

const undoBtn = () => screen.getByRole('button', { name: /undo annotation/i });
const redoBtn = () => screen.getByRole('button', { name: /redo annotation/i });

describe('the annotation undo/redo buttons', () => {
  it('start disabled on a paper nobody has annotated yet', () => {
    render(AnnotationHistory);
    expect(undoBtn()).toBeDisabled();
    expect(redoBtn()).toBeDisabled();
  });

  it('mirror the stack the document has', () => {
    state.canUndo = true;
    render(AnnotationHistory);
    expect(undoBtn()).toBeEnabled();
    expect(redoBtn()).toBeDisabled();
  });

  it('act through the shared commands, so the button and ⌘Z cannot diverge', async () => {
    state.canUndo = true;
    state.canRedo = true;
    render(AnnotationHistory);
    await userEvent.click(undoBtn());
    await userEvent.click(redoBtn());
    expect(scope.undo).toHaveBeenCalledOnce();
    expect(scope.redo).toHaveBeenCalledOnce();
  });
});
