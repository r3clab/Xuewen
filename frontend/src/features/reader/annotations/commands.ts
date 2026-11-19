/// Delete / undo / redo for the reader's marks, reachable from the global
/// keymap.
///
/// Same shape and same reason as pdfCopy.ts: the keymap owns every keystroke
/// over the reader, and the capabilities these commands need belong to
/// whichever document is the active tab when the key lands. So the policy
/// lives here, DOM-free and dependency-injected; PdfDeck (one live instance,
/// not one per tab) supplies the lookups; shortcuts.ts and keymap.ts call the
/// free functions at the bottom. The floating selection menu on the page calls
/// the same `deleteSelection`, so a mark removed with the trash button and one
/// removed with the Delete key take an identical path.
///
/// Nothing here may import pdf/engine.ts or any @embedpdf package at runtime:
/// app/keys/keymap.ts imports this module, app/App.svelte imports
/// app/keys/keymap.ts, and the whole reader tier is deliberately behind a
/// dynamic import in app/App.svelte. One plugin import here would drag all of
/// it into the library view's bundle.

/// A document's annotation capability, narrowed to what these commands use so
/// a test can hand it a small fake instead of whole `Annotation`s.
export interface MarkScope {
  readonly selection: {
    getSelected(): readonly unknown[];
    clear(): void;
    delete(): Promise<unknown>;
  };
}

/// A document's history capability, likewise narrowed. The annotation plugin
/// is the only registered plugin that records history, so the whole timeline
/// is the marks.
export interface HistoryScopeLike {
  undo(): void;
  redo(): void;
  canUndo(): boolean;
  canRedo(): boolean;
}

export interface AnnotationCommandOptions {
  /// The document's annotation capability, or null until it is ready.
  marks: (documentId: string) => MarkScope | null;
  /// The document's history capability, or null until it is ready.
  history: (documentId: string) => HistoryScopeLike | null;
  /// Read at call time, never captured: the active tab changes under us.
  activeDocumentId: () => string | null;
}

export interface AnnotationCommands {
  /// Whether a mark is selected in the active tab right now — the gate on the
  /// Delete/Backspace bindings.
  hasSelection(): boolean;
  /// Each command answers whether it actually did something, so the keymap can
  /// suppress the browser's own handling of the key only when it did, and
  /// otherwise leave the keystroke entirely alone.
  deleteSelection(): boolean;
  clearSelection(): boolean;
  undo(): boolean;
  redo(): boolean;
}

/// A capability whose document closes under a keystroke refuses with
/// `not-ready`; an exception escaping handleKeydown would take the rest of the
/// keymap down with it, so a refusal is a no-op instead.
function attempt<T>(run: () => T, fallback: T): T {
  try {
    return run();
  } catch {
    return fallback;
  }
}

export function createAnnotationCommands(opts: AnnotationCommandOptions): AnnotationCommands {
  function marks(): MarkScope | null {
    const id = opts.activeDocumentId();
    return id === null ? null : opts.marks(id);
  }

  function history(): HistoryScopeLike | null {
    const id = opts.activeDocumentId();
    return id === null ? null : opts.history(id);
  }

  function selectedCount(scope: MarkScope): number {
    return attempt(() => scope.selection.getSelected().length, 0);
  }

  return {
    hasSelection(): boolean {
      const scope = marks();
      return scope !== null && selectedCount(scope) > 0;
    },

    deleteSelection(): boolean {
      const scope = marks();
      if (!scope || selectedCount(scope) === 0) return false;
      // Through the plugin, never the store: the plugin records the undo step
      // and fires the delete events sync.ts turns into sidecar DELETEs.
      // Removing the row directly would leave the mark drawn on the page until
      // the tab was reopened. A refused delete puts the mark back by itself.
      return attempt(() => {
        void scope.selection.delete().catch(() => {});
        return true;
      }, false);
    },

    clearSelection(): boolean {
      const scope = marks();
      if (!scope || selectedCount(scope) === 0) return false;
      return attempt(() => {
        scope.selection.clear();
        return true;
      }, false);
    },

    undo(): boolean {
      const h = history();
      if (!h || !attempt(() => h.canUndo(), false)) return false;
      return attempt(() => {
        h.undo();
        return true;
      }, false);
    },

    redo(): boolean {
      const h = history();
      if (!h || !attempt(() => h.canRedo(), false)) return false;
      return attempt(() => {
        h.redo();
        return true;
      }, false);
    },
  };
}

/// The live commands, or null while no reader is mounted (the library view, and
/// every test that doesn't register its own). Every function below reads that
/// as "nothing to act on" and answers false, which is what keeps the bindings
/// inert outside the reader.
let current: AnnotationCommands | null = null;

export function registerAnnotationCommands(commands: AnnotationCommands | null): void {
  current = commands;
}

/// Clear on teardown by identity, not blindly: should a replacement PdfDeck
/// ever mount before the outgoing one tears down, a blind `register(null)`
/// would unregister the live commands and leave every binding silently inert.
export function unregisterAnnotationCommands(commands: AnnotationCommands): void {
  if (current === commands) current = null;
}

export function annotationSelectionActive(): boolean {
  return current?.hasSelection() ?? false;
}

export function deleteSelectedAnnotations(): boolean {
  return current?.deleteSelection() ?? false;
}

export function clearAnnotationSelection(): boolean {
  return current?.clearSelection() ?? false;
}

export function undoAnnotation(): boolean {
  return current?.undo() ?? false;
}

export function redoAnnotation(): boolean {
  return current?.redo() ?? false;
}
