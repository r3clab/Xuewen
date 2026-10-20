/// Delete / undo / redo for the reader's marks, reachable from the global
/// keymap.
///
/// Same shape and same reason as pdfCopy.ts: EmbedPDF binds no keys anywhere,
/// so every keystroke over the reader is this app's to handle, and the two
/// capabilities these commands need — annotations and history — are
/// registry-wide rather than per-tab. So the policy lives here, DOM-free and
/// dependency-injected; PdfDeck (one live instance, not one per tab) registers
/// the real capabilities; shortcuts.ts and keymap.ts call the free functions at
/// the bottom. The floating selection menu on the page calls the same
/// `deleteSelection`, so a mark removed with the trash button and one removed
/// with the Delete key take an identical path.
///
/// Nothing here may import pdf/engine.ts or any @embedpdf package:
/// app/keys/keymap.ts imports this module, app/App.svelte imports
/// app/keys/keymap.ts, and the whole reader tier (~5.8 MB) is deliberately
/// behind a dynamic import in app/App.svelte. One plugin import here would
/// drag all of it into the library view's bundle.
/// That is also why ANNOTATION_HISTORY_TOPIC lives in this module rather than
/// beside the plugin registration it belongs to.

/// The topic the annotation plugin files its undo/redo commands under. Scoping
/// undo to it means an undo — from the toolbar buttons or from ⌘Z — can only
/// ever take back a mark, never whatever a future plugin puts on the same
/// global timeline. The plugin keeps this private (`ANNOTATION_HISTORY_TOPIC`
/// in annotation-plugin.d.ts), so the string is version-pinned to
/// @embedpdf/plugin-annotation 2.14.4; a rename upstream shows up as undo that
/// does nothing, not as a wrong undo.
export const ANNOTATION_HISTORY_TOPIC = 'annotations';

/// The annotation plugin's per-document scope, narrowed to what these commands
/// use. Structural rather than the real `AnnotationScope` so a test can hand it
/// a two-field fake instead of building whole `PdfAnnotationObject`s — narrow
/// on purpose, the same reason sync.ts declares `SyncScope`. Note this
/// is NOT the case pdfCopy's `SelectionLike` and loadCitations' `EngineLike`
/// answer: those two exist because the real types don't assign at all. Both
/// real scopes here would; testability is the whole reason.
export interface MarkScope {
  getSelectedAnnotations(): { object: { id: string; pageIndex: number } }[];
  deleteAnnotations(annotations: { pageIndex: number; id: string }[]): void;
  deselectAnnotation(): void;
}

/// What one history topic can currently do. Absent from `topics` entirely
/// until that topic's first command, which is why every read below defaults it.
export interface HistoryFlags {
  canUndo: boolean;
  canRedo: boolean;
}

/// The history plugin's per-document scope, likewise narrowed.
export interface HistoryScopeLike {
  undo(topic?: string): void;
  redo(topic?: string): void;
  getHistoryState(): { topics: Record<string, HistoryFlags | undefined> };
}

export interface AnnotationCommandOptions {
  /// `annotationCapability.forDocument`.
  marks: (documentId: string) => MarkScope;
  /// `historyCapability.forDocument`.
  history: (documentId: string) => HistoryScopeLike;
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

/// Both plugins keep their per-document state in a map filled by
/// `onDocumentLoadingStarted`, and both THROW for an id they have not seen
/// ("Annotation state not found for document: <id>" / "History data not found
/// for document: <id>"). The active tab can genuinely be in that window: a tab
/// exists the moment it is opened, PdfDeck's openDocumentUrl resolves later,
/// and a restored session's background tabs wait for an idle callback. A
/// keystroke landing there has to be a no-op, because an exception would escape
/// handleKeydown and take the rest of the keymap down with it — the same trap
/// PdfPages.svelte documents for useAnnotation, where it blanked the reader.
function attempt<T>(run: () => T, fallback: T): T {
  try {
    return run();
  } catch {
    return fallback;
  }
}

/// The annotations topic's undo/redo flags. Stated once so the ⌘Z guard below
/// and the toolbar buttons that mirror it into runes (AnnotationHistory.svelte)
/// cannot disagree about the topic name or about what an absent topic means.
/// The answer for a document the plugin has never heard of, or a topic with no
/// commands yet: nothing to take back either way.
const DEAD_STACK: HistoryFlags = { canUndo: false, canRedo: false };

export function annotationHistoryFlags(history: HistoryScopeLike): HistoryFlags {
  return attempt(() => {
    const topic = history.getHistoryState().topics[ANNOTATION_HISTORY_TOPIC];
    return { canUndo: topic?.canUndo ?? false, canRedo: topic?.canRedo ?? false };
  }, DEAD_STACK);
}

export function createAnnotationCommands(opts: AnnotationCommandOptions): AnnotationCommands {
  /// The scopes for the tab a keystroke belongs to, or null on the library view.
  function active(): { marks: MarkScope; history: HistoryScopeLike } | null {
    const id = opts.activeDocumentId();
    if (id === null) return null;
    return { marks: opts.marks(id), history: opts.history(id) };
  }

  /// The selection as the delete call wants it. An array, not a single mark:
  /// the plugin supports multi-select, and deleting only the first would leave
  /// the rest drawn with no way to reach them.
  function selected(marks: MarkScope): { pageIndex: number; id: string }[] {
    return attempt(
      () =>
        marks
          .getSelectedAnnotations()
          .map((s) => ({ pageIndex: s.object.pageIndex, id: s.object.id })),
      [],
    );
  }

  return {
    hasSelection(): boolean {
      const a = active();
      return a !== null && selected(a.marks).length > 0;
    },

    deleteSelection(): boolean {
      const a = active();
      if (!a) return false;
      const marks = selected(a.marks);
      if (marks.length === 0) return false;
      // Through the plugin, never the store: the plugin registers the undo
      // command and emits the delete events sync.ts turns into sidecar
      // DELETEs. Removing the row directly would leave the mark drawn on the
      // page until the tab was reopened. One call, but the plugin loops
      // internally and registers one undo command PER mark, so taking back a
      // multi-mark delete costs a ⌘Z each — only reachable from the Delete key,
      // since the floating menu never renders for a multi-selection.
      return attempt(() => {
        a.marks.deleteAnnotations(marks);
        return true;
      }, false);
    },

    clearSelection(): boolean {
      const a = active();
      if (!a || selected(a.marks).length === 0) return false;
      return attempt(() => {
        a.marks.deselectAnnotation();
        return true;
      }, false);
    },

    undo(): boolean {
      const a = active();
      if (!a || !annotationHistoryFlags(a.history).canUndo) return false;
      return attempt(() => {
        a.history.undo(ANNOTATION_HISTORY_TOPIC);
        return true;
      }, false);
    },

    redo(): boolean {
      const a = active();
      if (!a || !annotationHistoryFlags(a.history).canRedo) return false;
      return attempt(() => {
        a.history.redo(ANNOTATION_HISTORY_TOPIC);
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

/// Clear on teardown — by identity, not blindly. PdfDeck is not mounted once
/// despite reading like it is: `<EmbedPDF>` renders its children twice during
/// startup and destroys the first subtree (see `documentsToAdopt` in
/// pdf/deck.ts). Today the old instance is torn down before the new one mounts,
/// so a blind `register(null)` would happen to be harmless; if that order ever
/// flips, it would unregister the live commands and leave every binding
/// silently inert.
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
