import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const ref = { kind: 'objectNumber', page: { kind: 'objectNumber', objectNumber: 3 }, objectNumber: 9 };
const annotationState = { selected: [] as { subtype: string; ref: typeof ref }[] };
const canDelete = vi.fn(() => true);

vi.mock('@embedpdf/svelte/annotation', () => ({
  useAnnotation: () => ({ canDelete }),
  useAnnotationState: () => annotationState,
}));

import AnnotationSelectionMenu from './AnnotationSelectionMenu.svelte';
import { registerAnnotationCommands, type AnnotationCommands } from './commands';

const deleteSelection = vi.fn(() => true);
const commands: AnnotationCommands = {
  hasSelection: () => true,
  deleteSelection,
  clearSelection: () => true,
  undo: () => true,
  redo: () => true,
};

function select(...subtypes: string[]): void {
  annotationState.selected = subtypes.map((subtype) => ({ subtype, ref }));
}

beforeEach(() => {
  deleteSelection.mockClear();
  canDelete.mockReturnValue(true);
  registerAnnotationCommands(commands);
});

// Registered globally by PdfDeck in the app; must not leak into sibling files.
afterEach(() => registerAnnotationCommands(null));

describe('the selected mark’s floating menu', () => {
  it('deletes through the same command the Delete key uses', async () => {
    select('highlight');
    render(AnnotationSelectionMenu);
    await userEvent.click(screen.getByRole('button', { name: /delete highlight/i }));
    expect(deleteSelection).toHaveBeenCalledTimes(1);
  });

  it('falls back to a generic name for a subtype this app never stores', () => {
    // A mark baked into the PDF by another reader — visible and selectable,
    // but not one of the five kinds the sidecar knows how to name.
    select('ink');
    render(AnnotationSelectionMenu);
    expect(screen.getByRole('button', { name: /delete annotation/i })).toBeInTheDocument();
  });

  it('offers nothing for a mark the reader may not delete', () => {
    select('highlight');
    canDelete.mockReturnValue(false);
    render(AnnotationSelectionMenu);
    expect(canDelete).toHaveBeenCalledWith(ref);
    expect(screen.queryByRole('button')).toBeNull();
  });

  it('offers nothing for a multi-selection', () => {
    select('highlight', 'underline');
    render(AnnotationSelectionMenu);
    expect(screen.queryByRole('button')).toBeNull();
  });
});
