/// ⌘C over the reader. Copying a PDF selection is entirely this app's job:
/// EmbedPDF binds no keys anywhere, and the only clipboard write in the whole
/// package tree lives in an auto-mounted utility we deliberately don't register
/// (see pdf/engine.ts). The browser can't do it either — a page is an <img>
/// (RenderLayer/TilingLayer) and the selection overlay is empty pointer-events:
/// none <div>s, so the document selection over a page is always collapsed, the
/// copy command stays disabled, and no `copy` event is ever dispatched. There
/// is nothing to intercept; the keystroke has to be handled outright.
///
/// The text therefore has to be in hand BEFORE the keystroke. getSelectedText
/// is a PDFium `getTextSlices` round-trip through the worker, i.e. a full task
/// boundary, and a clipboard write on the far side of one has lost the user
/// gesture that authorized it (WebKit/WKWebView enforces this strictly, and
/// copyText's execCommand fallback needs the same live gesture). So the text is
/// fetched eagerly the moment a selection settles and cached here, and
/// `copySelection` writes it with no await in front of the write.
///
/// Kept DOM-free and dependency-injected so the whole policy is unit-testable;
/// PdfDeck supplies the real capability and `copyText`.

/// The plugin surface this module needs, declared structurally rather than
/// imported as `SelectionCapability`. Two reasons, both load-bearing: the real
/// `PdfTask` aliases a class with private fields, so a `{ toPromise }` fake can
/// never satisfy it and `npm run check` fails on the test (the same reason
/// citations/load.ts declares `EngineLike`); and the capability's event hooks
/// are typed `EventHook<T>`, a *union* of two call signatures, which does not
/// assign to a plain method under strictFunctionTypes.
export interface SelectionLike {
  getSelectedText(documentId: string): { toPromise(): Promise<string[]> };
  onBeginSelection(handler: (ev: { documentId: string }) => void): () => void;
  onSelectionChange(handler: (ev: { documentId: string; selection: unknown }) => void): () => void;
  onEndSelection(handler: (ev: { documentId: string }) => void): () => void;
}

export interface PdfCopyOptions {
  selection: SelectionLike;
  /// Read at call time, never captured: the active tab changes under us.
  activeDocumentId: () => string | null;
  /// Production: `copyText` from clipboard.ts (Clipboard API, falling back
  /// to a hidden textarea + execCommand for plain-HTTP `--allow-remote`).
  copy: (text: string) => Promise<void>;
  /// Drops any native DOM selection when a PDF selection starts, so the two can
  /// never both be live and the ⌘C guard stays unambiguous. Production:
  /// `() => document.getSelection()?.removeAllRanges()`.
  clearNativeSelection?: () => void;
  onError?: (message: string) => void;
}

export interface PdfCopy {
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
/// (PdfPages). The copier already runs the one reliable settle detector (the
/// debounce below) and already holds the fetched text, so consumers subscribe
/// here instead of duplicating both and paying a second PDFium round-trip per
/// selection. Fired with the '\n'-joined text once a fetch lands, or with ''
/// when a fetch comes back empty (the selection was cleared); see prefetch for
/// the exact duplicate-suppression rule. Module-level like registerPdfCopy:
/// listeners outlive any one copier instance.
export type SettledSelectionListener = (documentId: string, text: string) => void;
const settledListeners = new Set<SettledSelectionListener>();

export function onPdfSelectionSettled(listener: SettledSelectionListener): () => void {
  settledListeners.add(listener);
  return () => settledListeners.delete(listener);
}

/// How long the selection must stop changing before we fetch its text.
///
/// This settle timer — not `onEndSelection` — is what makes the feature
/// reliable. The plugin registers its text handler per page
/// (`registerAlways({ scope: { type: 'page', … } })`, plugin-selection
/// 2.14.4 index.js:941) and keeps `dragStarted` in that page's own closure, so
/// `onEnd` fires only when the pointer is released over the very page the drag
/// began on. Releasing in the gutter between pages, past the page margin, over
/// the floating toolbar, or on a second page — i.e. most multi-page and
/// select-to-end-of-paragraph drags — ends with a live, visibly highlighted
/// selection and no end event at all. `onSelectionChange` does fire for every
/// glyph the range moves over (index.js:1262), so a trailing debounce on it
/// catches every one of those cases. `onEndSelection` is kept as the fast path
/// for the common release-on-page drag and for double/triple-click, which
/// emits it with no pointerup at all (index.js:1247).
const SETTLE_MS = 200;

export function createPdfCopy(opts: PdfCopyOptions): PdfCopy {
  const { selection, activeDocumentId, copy, clearNativeSelection, onError } = opts;

  // All three are keyed by documentId, never a single "last selection" slot:
  // selection state lives per document in the plugin and survives a tab switch
  // (only onDocumentClosed tears it down), so a background tab's still-
  // highlighted selection has to stay copyable when the user switches back.
  const cache = new Map<string, string>();
  const inflight = new Map<string, Promise<string>>();
  const timers = new Map<string, ReturnType<typeof setTimeout>>();

  function cancelSettle(documentId: string): void {
    const timer = timers.get(documentId);
    if (timer !== undefined) {
      clearTimeout(timer);
      timers.delete(documentId);
    }
  }

  /// Both rejection arms collapse to '': Security (the document denies
  /// CopyContents — normally pre-empted by the registry override in
  /// pdf/engine.ts) and NotFound (the selection was cleared before the fetch
  /// landed). Neither is copyable, and swallowing them here is also what keeps
  /// the eager prefetch from raising an unhandled rejection.
  function fetchText(documentId: string): Promise<string> {
    return selection
      .getSelectedText(documentId)
      .toPromise()
      .then(
        (parts) => (parts ?? []).join('\n'),
        () => '',
      )
      .then((text) => (text.trim() ? text : ''));
  }

  function prefetch(documentId: string): void {
    cancelSettle(documentId);
    const done = fetchText(documentId);
    inflight.set(documentId, done);
    void done.then((text) => {
      // Identity check, not presence: a newer selection on this same document
      // has already replaced the entry, and its result must win however the
      // two round-trips happen to interleave.
      if (inflight.get(documentId) !== done) return;
      inflight.delete(documentId);
      const prev = cache.get(documentId) ?? '';
      if (text) cache.set(documentId, text);
      else cache.delete(documentId);
      // Settled-selection feed (selection→translate). Non-empty text notifies
      // only when it differs from what was already cached: a release-on-page
      // drag whose settle timer already fetched fires prefetch AGAIN from
      // onEndSelection, and re-announcing the identical text would fire a
      // second (paid) translation for one selection. Empty text always
      // notifies — it is how a click that cleared the selection dismisses the
      // consumer's UI, and dismissal is idempotent.
      if (text === '' || text !== prev) {
        for (const listener of settledListeners) listener(documentId, text);
      }
    });
  }

  // A PDF selection is starting, so any native selection elsewhere in the app
  // is stale. Dropping it here means `hasDomSelection` in shortcuts.ts can
  // treat "a real DOM selection exists" as an unambiguous signal to stand aside
  // and let the browser copy natively.
  const unsubBegin = selection.onBeginSelection((ev) => {
    if (ev.documentId !== activeDocumentId()) return;
    clearNativeSelection?.();
  });

  // Fires on every glyph the range grows by, and with a null range on the
  // pointerdown that clears it. Either way the cached text is stale as of this
  // instant, so it is dropped first and only re-fetched once the range settles.
  const unsubChange = selection.onSelectionChange((ev) => {
    cache.delete(ev.documentId);
    inflight.delete(ev.documentId);
    cancelSettle(ev.documentId);
    if (!ev.selection || ev.documentId !== activeDocumentId()) return;
    timers.set(
      ev.documentId,
      setTimeout(() => {
        timers.delete(ev.documentId);
        prefetch(ev.documentId);
      }, SETTLE_MS),
    );
  });

  const unsubEnd = selection.onEndSelection((ev) => {
    if (ev.documentId !== activeDocumentId()) return;
    prefetch(ev.documentId);
  });

  function write(text: string): Promise<void> {
    return copy(text).catch(() => {
      onError?.("Couldn't copy");
    });
  }

  return {
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
      // pending settle timer (no double round-trip) and warms the cache for
      // the retry. Chrome's transient activation (~5s) survives the round-trip;
      // WebKit's may not, in which case the error toast fires and a second ⌘C,
      // now warm, takes the synchronous path above.
      if (!inflight.has(id)) prefetch(id);
      const text = await inflight.get(id);
      if (text) await write(text);
    },

    forget(documentId: string): void {
      cache.delete(documentId);
      inflight.delete(documentId);
      cancelSettle(documentId);
    },

    destroy(): void {
      unsubBegin();
      unsubChange();
      unsubEnd();
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
      cache.clear();
      inflight.clear();
    },
  };
}

// The live instance, published by PdfDeck. shortcuts.ts is a plain module with
// no access to the EmbedPDF context — same division of labour as
// state.svelte.ts's openFind, which the keymap calls into rather than reaching
// into the reader.
let current: PdfCopy | null = null;

export function registerPdfCopy(copier: PdfCopy | null): void {
  current = copier;
}

/// Clear on teardown — by identity, not blindly, exactly as
/// unregisterAnnotationCommands does and for the same reason: PdfDeck reads
/// like it mounts once but is destroyed and remounted during startup (see
/// documentsToAdopt in pdf/deck.ts). Today the outgoing instance tears down
/// before the replacement mounts, so a blind clear would happen to be
/// harmless; if that order ever flips it would unregister the live copier and
/// leave ⌘C silently dead over the reader.
export function unregisterPdfCopy(copier: PdfCopy): void {
  if (current === copier) current = null;
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
