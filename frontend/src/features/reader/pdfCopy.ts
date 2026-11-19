/// ⌘C over the reader. Copying a PDF selection is entirely this app's job:
/// a page is pixels with no HTML text under it, so the document selection over
/// one is always collapsed, the copy command stays disabled, and no `copy`
/// event is ever dispatched. EmbedPDF's own <SelectionClipboard> is not
/// mounted: the keymap (app/keys/shortcuts.ts) owns ⌘C, and the text fetched
/// here is also what selection→translate reads (onPdfSelectionSettled), so one
/// read serves both.
///
/// The text therefore has to be in hand BEFORE the keystroke. `readText` reads
/// each page's text from the engine's worker on first use, a full task
/// boundary, and a clipboard write on the far side of one has lost the user
/// gesture that authorized it (WebKit/WKWebView enforces this strictly, and
/// copyText's execCommand fallback needs the same live gesture). So the text is
/// fetched eagerly the moment a selection settles and cached here, and
/// `copySelection` writes it with no await in front of the write.
///
/// Kept DOM-free and dependency-injected so the whole policy is unit-testable;
/// PdfDeck supplies `copyText`, and each document's view attaches that
/// document's selection (attachPdfSelection).

/// What this module reads off a document's `SelectionCapability`.
export interface SelectionSource {
  readText(): Promise<string>;
  onChanged(listener: (event: { range: unknown }) => void): () => void;
  onCommitted(listener: () => void): () => void;
}

export interface PdfCopyOptions {
  /// Read at call time, never captured: the active tab changes under us.
  activeDocumentId: () => string | null;
  /// Production: `copyText` from clipboard.ts (Clipboard API, falling back
  /// to a hidden textarea + execCommand for plain-HTTP `--allow-remote`).
  copy: (text: string) => Promise<void>;
  /// Drops any native DOM selection while a PDF selection is being made, so
  /// the two can never both be live and the ⌘C guard stays unambiguous.
  /// Production: `() => document.getSelection()?.removeAllRanges()`.
  clearNativeSelection?: () => void;
  onError?: (message: string) => void;
}

export interface PdfCopy {
  /// Follow one document's selection until the returned function is called.
  attach(documentId: string, source: SelectionSource): () => void;
  /// Whether ⌘C should be taken over for the active tab right now.
  hasSelection(): boolean;
  /// Whether the settled-selection feed still owes this document an
  /// announcement (a settle timer armed or a fetch in flight) — i.e. any text
  /// a consumer holds from an earlier announcement is stale.
  fetchPending(documentId: string): boolean;
  copySelection(): Promise<void>;
  /// Drop a closed document's cached text (PdfDeck's close loop).
  forget(documentId: string): void;
  destroy(): void;
}

/// Consumers of the settled selection beyond ⌘C — today, selection→translate
/// (PdfPages). The copier already runs the settle detector (the debounce
/// below) and already holds the fetched text, so consumers subscribe here
/// instead of duplicating both and paying a second text read per selection.
/// Fired with the text once a fetch lands, or with '' when the selection is
/// cleared or a fetch comes back empty; see prefetch for the exact
/// duplicate-suppression rule. Module-level like registerPdfCopy: listeners
/// outlive any one copier instance.
export type SettledSelectionListener = (documentId: string, text: string) => void;
const settledListeners = new Set<SettledSelectionListener>();

export function onPdfSelectionSettled(listener: SettledSelectionListener): () => void {
  settledListeners.add(listener);
  return () => settledListeners.delete(listener);
}

function announce(documentId: string, text: string): void {
  for (const listener of settledListeners) listener(documentId, text);
}

/// How long the selection must stop changing before we fetch its text.
/// `onCommitted` is the fast path for a finished drag, double- or
/// triple-click; this trailing debounce on `onChanged` catches every change
/// that ends without one — the range moving again once a boundary page's text
/// arrives after the release, or a long pause mid-drag.
const SETTLE_MS = 200;

export function createPdfCopy(opts: PdfCopyOptions): PdfCopy {
  const { activeDocumentId, copy, clearNativeSelection, onError } = opts;

  // All keyed by documentId, never a single "last selection" slot: selection
  // state lives per document in the plugin and survives a tab switch, so a
  // background tab's still-highlighted selection has to stay copyable when the
  // user switches back.
  const sources = new Map<string, SelectionSource>();
  const cache = new Map<string, string>();
  const inflight = new Map<string, Promise<string>>();
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  const detachers = new Set<() => void>();

  function cancelSettle(documentId: string): void {
    const timer = timers.get(documentId);
    if (timer !== undefined) {
      clearTimeout(timer);
      timers.delete(documentId);
    }
  }

  function forget(documentId: string): void {
    cache.delete(documentId);
    inflight.delete(documentId);
    cancelSettle(documentId);
  }

  /// A rejection collapses to '': the read was refused (a document that
  /// forbids copying — normally pre-empted by DOCUMENT_SCOPE in pdf/engine.ts)
  /// or cancelled because the document closed. Neither is copyable, and
  /// swallowing it here is also what keeps the eager prefetch from raising an
  /// unhandled rejection.
  function fetchText(documentId: string): Promise<string> {
    const source = sources.get(documentId);
    if (!source) return Promise.resolve('');
    return source.readText().then(
      (text) => (text.trim() ? text : ''),
      () => '',
    );
  }

  function prefetch(documentId: string): void {
    cancelSettle(documentId);
    const done = fetchText(documentId);
    inflight.set(documentId, done);
    void done.then((text) => {
      // Identity check, not presence: a newer selection on this same document
      // has already replaced the entry, and its result must win however the
      // two reads happen to interleave.
      if (inflight.get(documentId) !== done) return;
      inflight.delete(documentId);
      const prev = cache.get(documentId) ?? '';
      if (text) cache.set(documentId, text);
      else cache.delete(documentId);
      // Non-empty text notifies only when it differs from what was already
      // cached: a drag whose settle timer already fetched mid-gesture fires
      // prefetch AGAIN from onCommitted, and re-announcing the identical text
      // would fire a second (paid) translation for one selection. Empty text
      // always notifies; dismissal is idempotent.
      if (text === '' || text !== prev) announce(documentId, text);
    });
  }

  function attach(documentId: string, source: SelectionSource): () => void {
    sources.set(documentId, source);

    // Fires on every step the range moves, and with a null range when it is
    // cleared. Either way the cached text is stale as of this instant, so it is
    // dropped first and only re-fetched once the range settles.
    const offChanged = source.onChanged(({ range }) => {
      forget(documentId);
      if (!range) {
        // How a click that cleared the selection dismisses the consumer's UI:
        // a clear ends no gesture with a selection, so onCommitted never comes.
        announce(documentId, '');
        return;
      }
      if (documentId !== activeDocumentId()) return;
      // `hasDomSelection` in shortcuts.ts can then treat "a real DOM selection
      // exists" as an unambiguous signal to stand aside and let the browser
      // copy natively.
      clearNativeSelection?.();
      timers.set(
        documentId,
        setTimeout(() => {
          timers.delete(documentId);
          prefetch(documentId);
        }, SETTLE_MS),
      );
    });

    const offCommitted = source.onCommitted(() => {
      if (documentId === activeDocumentId()) prefetch(documentId);
    });

    const detach = (): void => {
      if (!detachers.delete(detach)) return;
      offChanged();
      offCommitted();
      if (sources.get(documentId) === source) {
        sources.delete(documentId);
        forget(documentId);
      }
    };
    detachers.add(detach);
    return detach;
  }

  function write(text: string): Promise<void> {
    return copy(text).catch(() => {
      onError?.("Couldn't copy");
    });
  }

  return {
    attach,

    hasSelection(): boolean {
      const id = activeDocumentId();
      if (!id) return false;
      // A pending settle counts: the user has a selection on screen and simply
      // beat the timer to the keystroke. Claiming ⌘C here sends them down
      // copySelection's cold path rather than letting the browser no-op.
      return cache.has(id) || inflight.has(id) || timers.has(id);
    },

    fetchPending(documentId: string): boolean {
      return timers.has(documentId) || inflight.has(documentId);
    },

    async copySelection(): Promise<void> {
      const id = activeDocumentId();
      if (!id) return;
      const cached = cache.get(id);
      // The whole point of the eager fetch: `write` is reached synchronously,
      // inside the keydown, while the user gesture still authorizes a clipboard
      // write. Never put an await in front of this line.
      if (cached) return write(cached);
      // Cold or still in flight — the user out-raced the prefetch. Reuse the
      // in-flight fetch, or start one through prefetch so it also cancels any
      // pending settle timer (no double read) and warms the cache for the
      // retry. Chrome's transient activation (~5s) survives the read; WebKit's
      // may not, in which case the error toast fires and a second ⌘C, now
      // warm, takes the synchronous path above.
      if (!inflight.has(id)) prefetch(id);
      const text = await inflight.get(id);
      if (text) await write(text);
    },

    forget,

    destroy(): void {
      for (const detach of [...detachers]) detach();
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
      cache.clear();
      inflight.clear();
    },
  };
}

// The live instance, published by PdfDeck. shortcuts.ts is a plain module with
// no access to the viewer — same division of labour as state.svelte.ts's
// openFind, which the keymap calls into rather than reaching into the reader.
let current: PdfCopy | null = null;

export function registerPdfCopy(copier: PdfCopy | null): void {
  current = copier;
}

/// Clear on teardown by identity, not blindly, exactly as
/// unregisterAnnotationCommands does: should a replacement PdfDeck ever mount
/// before the outgoing one tears down, a blind clear would unregister the live
/// copier and leave ⌘C silently dead over the reader.
export function unregisterPdfCopy(copier: PdfCopy): void {
  if (current === copier) current = null;
}

/// Called by a document's view once its selection is ready. PdfDeck registers
/// the copier before any view mounts, so there is always one to attach to.
export function attachPdfSelection(documentId: string, source: SelectionSource): () => void {
  return current?.attach(documentId, source) ?? (() => {});
}

export function pdfSelectionHasText(): boolean {
  return current?.hasSelection() ?? false;
}

/// `fetchPending` on the live copier. PdfPages' pointerup uses it to drop a
/// mid-drag parked settle: the drag kept moving after that text was fetched,
/// so the pending fetch announces the final text itself (the pointer is up by
/// then) and firing the parked copy too would pay for a second translation of
/// one selection — the first with text the user never finally selected. No
/// copier means nothing was ever announced or parked, so false is safe.
export function pdfSelectionFetchPending(documentId: string): boolean {
  return current?.fetchPending(documentId) ?? false;
}

export function copyPdfSelection(): void {
  void current?.copySelection();
}

export function forgetPdfSelection(documentId: string): void {
  current?.forget(documentId);
}
