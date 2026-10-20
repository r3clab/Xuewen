import { render, screen } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PdfAnnotationSubtype } from '@embedpdf/models';
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

/// The plugin's wrapper props. The real action stops pointerdown at capture so
/// a click in the menu can't reach the page and deselect the mark first; here
/// it only has to be a valid action.
const menuWrapperProps = {
  style: 'position: absolute; left: 10px; top: 20px',
  action: () => ({ destroy: () => {} }),
};

function props(over: { structurallyLocked?: boolean; type?: PdfAnnotationSubtype } = {}) {
  return {
    menuWrapperProps,
    context: {
      type: 'annotation' as const,
      pageIndex: 2,
      structurallyLocked: over.structurallyLocked ?? false,
      contentLocked: false,
      annotation: {
        object: { type: over.type ?? PdfAnnotationSubtype.HIGHLIGHT },
      },
    },
  } as unknown as import('svelte').ComponentProps<typeof AnnotationSelectionMenu>;
}

beforeEach(() => {
  deleteSelection.mockClear();
  registerAnnotationCommands(commands);
});

// Registered globally by PdfDeck in the app; must not leak into sibling files.
afterEach(() => registerAnnotationCommands(null));

describe('the selected mark’s floating menu', () => {
  it('deletes through the same command the Delete key uses', async () => {
    render(AnnotationSelectionMenu, { props: props() });
    await userEvent.click(screen.getByRole('button', { name: /delete highlight/i }));
    expect(deleteSelection).toHaveBeenCalledTimes(1);
  });

  it('falls back to a generic name for a subtype this app never stores', () => {
    // A mark baked into the PDF by another reader — visible and selectable,
    // but not one of the five kinds the sidecar knows how to name.
    render(AnnotationSelectionMenu, {
      props: props({ type: PdfAnnotationSubtype.INK }),
    });
    expect(screen.getByRole('button', { name: /delete annotation/i })).toBeInTheDocument();
  });

  it('offers nothing for a mark the PDF has locked', () => {
    render(AnnotationSelectionMenu, {
      props: props({ structurallyLocked: true }),
    });
    expect(screen.queryByRole('button')).toBeNull();
  });
});
