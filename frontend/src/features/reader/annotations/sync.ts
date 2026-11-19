/// Keeps one open document's marks and the SQLite sidecar in step.
///
/// Ownership is the load-bearing idea. A PDF can arrive with annotations
/// already baked in by some other reader, and the plugin surfaces those exactly
/// like ours. If the save loop adopted them, the first save would copy them into
/// the sidecar, the next load would import them *on top of* the originals, and
/// the reader would show every one of them twice. So this tracks which marks
/// are ours — the rows we restored, plus the marks the user drew this session —
/// and persists nothing else. Foreign marks stay visible but read-only in
/// effect: the library file is never written, so an edit to one was never going
/// to be kept anyway.
///
/// A row's id is the mark's name (`nm`), which an import with
/// `attribution: 'restore'` gives back unchanged. Its `ref` is only the object
/// the engine wrote it as in this session, so the events' refs are mapped to
/// names as they arrive.

import type { AnnotationBundle, AnnotationDraft, AnnotationRef } from '@embedpdf/svelte/annotation';
import { annotationKey, type PageRef } from '@embedpdf/svelte/runtime';
import type { Quad } from '$features/reader/pdf/pageText';
import { errorMessage } from '$shared/errors';
import {
  RESTORE_OPTIONS,
  kindOf,
  planRestore,
  quadsOf,
  sameAsStored,
  toWire,
  type MarkData,
  type RestorePage,
} from './adapter';
import {
  annotations,
  annotationList,
  isLoaded,
  loadAnnotations,
  removeAnnotation,
  saveAnnotation,
} from './store.svelte';

/// The document's annotation capability, narrowed to what this needs. Narrow
/// on purpose: it is what lets the loop be tested without a PDF engine.
export interface SyncScope {
  get(ref: AnnotationRef): MarkData | null;
  list(): readonly MarkData[];
  import(
    bundle: AnnotationBundle,
    options: typeof RESTORE_OPTIONS,
  ): Promise<{ readonly annotations: readonly MarkData[] }>;
  create(page: PageRef, draft: AnnotationDraft): Promise<{ readonly annotation: MarkData }>;
  onCreated(listener: (event: { readonly annotation: MarkData }) => void): () => void;
  onUpdated(listener: (event: { readonly annotation: MarkData }) => void): () => void;
  onDeleted(listener: (event: { readonly refs: readonly AnnotationRef[] }) => void): () => void;
  onResynced(listener: (event: { readonly pages: readonly PageRef[] | 'all' }) => void): () => void;
}

/// The document's pages and their text.
export interface SyncPages {
  list(): readonly RestorePage[];
  indexOf(ref: PageRef): number;
  textUnder(page: PageRef, quads: readonly Quad[]): Promise<string>;
}

export interface AnnotationSyncOptions {
  paperId: string;
  scope: SyncScope;
  pages: SyncPages;
  /// Forget the document's undo steps. Redrawing a row the 2.x reader saved
  /// is a create, which records one; ⌘Z must not take back a mark the reader
  /// never drew this session.
  clearHistory: () => void;
  /// Called when a write fails, so the reader can say a mark did not save
  /// rather than losing it silently.
  onError?: (message: string) => void;
  debounceMs?: number;
}

export interface AnnotationSync {
  /// Load the sidecar and restore it into the document. Resolves once the
  /// marks are in.
  start(): Promise<void>;
  /// Write anything still pending. Resolves when the writes settle.
  flush(): Promise<void>;
  /// Flush, then stop listening. Closing a tab must not drop the last edit.
  destroy(): Promise<void>;
}

const DEFAULT_DEBOUNCE_MS = 500;

export function createAnnotationSync(opts: AnnotationSyncOptions): AnnotationSync {
  const { paperId, scope, pages } = opts;
  const wait = opts.debounceMs ?? DEFAULT_DEBOUNCE_MS;

  /// Every name that is ours this session, kept after its mark is deleted so a
  /// mark that undo brings back is still recognised.
  const owned = new Set<string>();
  /// Where each of our marks in the document is right now.
  const live = new Map<string, AnnotationRef>();
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  /// Writes in flight, so `flush` can wait for them rather than just for the
  /// timers it cleared.
  const inFlight = new Set<Promise<void>>();
  let stopped = false;
  /// One message per burst: a failing endpoint would otherwise raise a toast
  /// per mark per keystroke.
  let reportedError = false;

  function report(e: unknown): void {
    if (reportedError) return;
    reportedError = true;
    opts.onError?.(errorMessage(e, 'the server did not answer'));
  }

  function track(annotation: MarkData): void {
    if (annotation.nm) live.set(annotation.nm, annotation.ref);
  }

  /// One mark's writes, in order. Undo and redo turn a create into a delete and
  /// back within a click or two, and two requests racing on the same id could
  /// land the PUT before the DELETE — leaving the reader looking at a mark the
  /// sidecar has forgotten. Queued work reads the engine and the cache when it
  /// runs rather than when it was queued, so a save behind a delete sees the
  /// row is gone and writes it back instead of calling it unchanged. A mark
  /// with nothing pending still goes out synchronously.
  const chains = new Map<string, Promise<void>>();

  function enqueue(id: string, run: () => Promise<void>): void {
    const prev = chains.get(id);
    const done = (prev ? prev.then(run) : run()).then(
      () => {
        reportedError = false;
      },
      (e) => report(e),
    );
    chains.set(id, done);
    inFlight.add(done);
    void done.finally(() => {
      inFlight.delete(done);
      // Only if nothing queued behind it — otherwise the next write would lose
      // the predecessor it has to wait for.
      if (chains.get(id) === done) chains.delete(id);
    });
  }

  function write(id: string): void {
    enqueue(id, async () => {
      const ref = live.get(id);
      const current = ref ? scope.get(ref) : null;
      // Gone between the edit and the write — the delete path already ran.
      if (!current) return;
      const pageIndex = pages.indexOf(current.ref.page);
      if (pageIndex < 0) return;
      const quads = quadsOf(current);
      // A mark whose passage can't be read still saves, just without its quote.
      const quoted = quads.length > 0 ? await pages.textUnder(current.ref.page, quads).catch(() => '') : '';
      const body = toWire(current, pageIndex, quoted);
      // Defense in depth: ownership should already have excluded this.
      if (!body) return;
      const stored = annotations.byPaper[paperId]?.[id];
      // A move and a recolor both show up in the payload, so an unchanged mark
      // really is unchanged — worth checking, since every restored mark comes
      // back through onCreated.
      if (stored && sameAsStored(body, stored)) return;
      await saveAnnotation(paperId, id, body);
    });
  }

  function schedule(id: string): void {
    if (stopped) return;
    clearTimeout(timers.get(id));
    timers.set(
      id,
      setTimeout(() => {
        timers.delete(id);
        write(id);
      }, wait),
    );
  }

  function cancel(id: string): void {
    clearTimeout(timers.get(id));
    timers.delete(id);
  }

  /// The mark is ours: it was drawn here or restored from the sidecar.
  function onCreated({ annotation }: { readonly annotation: MarkData }): void {
    if (stopped) return;
    // The subtype whitelist is what keeps a tool we never surfaced — or a
    // reply, a popup, a widget — out of the sidecar.
    if (!kindOf(annotation) || !annotation.nm) return;
    owned.add(annotation.nm);
    track(annotation);
    // Written straight away rather than debounced: a mark is final the
    // moment it is drawn, and the sooner it is durable the better.
    write(annotation.nm);
  }

  function onUpdated({ annotation }: { readonly annotation: MarkData }): void {
    if (stopped || !annotation.nm || !owned.has(annotation.nm)) return;
    track(annotation);
    schedule(annotation.nm);
  }

  function removed(id: string): void {
    live.delete(id);
    cancel(id);
    enqueue(id, () => removeAnnotation(paperId, id));
  }

  function onDeleted({ refs }: { readonly refs: readonly AnnotationRef[] }): void {
    if (stopped) return;
    const keys = new Set(refs.map(annotationKey));
    for (const [id, ref] of live) if (keys.has(annotationKey(ref))) removed(id);
  }

  /// Undo of a delete and redo of a create bring a mark back with no create
  /// event, only a resync of its page; anything of ours that came back is
  /// written again, and anything that went is removed.
  function onResynced({ pages: changed }: { readonly pages: readonly PageRef[] | 'all' }): void {
    if (stopped) return;
    const onChanged = (page: PageRef): boolean =>
      changed === 'all' || changed.some((p) => p.objectNumber === page.objectNumber);
    const present = new Set<string>();
    for (const annotation of scope.list()) {
      const id = annotation.nm;
      if (!id || !owned.has(id) || !onChanged(annotation.ref.page)) continue;
      present.add(id);
      const back = !live.has(id);
      track(annotation);
      if (back) write(id);
      else schedule(id);
    }
    for (const [id, ref] of live) if (!present.has(id) && onChanged(ref.page)) removed(id);
  }

  async function restore(rows: ReturnType<typeof annotationList>): Promise<void> {
    const plan = planRestore(rows, pages.list());
    for (const id of plan.ids) owned.add(id);
    if (plan.bundle) {
      const { annotations: restored } = await scope.import(plan.bundle, RESTORE_OPTIONS);
      for (const annotation of restored) track(annotation);
    }
    if (plan.legacy.length > 0) {
      for (const { page, draft } of plan.legacy) {
        if (stopped) return;
        const { annotation } = await scope.create(page, draft);
        track(annotation);
      }
      opts.clearHistory();
    }
  }

  const unsubscribes: (() => void)[] = [];

  return {
    async start(): Promise<void> {
      unsubscribes.push(
        scope.onCreated(onCreated),
        scope.onUpdated(onUpdated),
        scope.onDeleted(onDeleted),
        scope.onResynced(onResynced),
      );
      await loadAnnotations(paperId);
      if (stopped) return;
      // A failed load leaves this paper's marks UNKNOWN, which is not the
      // same as none (the store's `loaded` contract): skip the restore — its
      // empty list would claim there is nothing — while the subscriptions
      // above keep marks drawn THIS session saving normally.
      if (!isLoaded(paperId)) return;
      await restore(annotationList(paperId)).catch(report);
    },

    async flush(): Promise<void> {
      for (const [id, timer] of timers) {
        clearTimeout(timer);
        timers.delete(id);
        write(id);
      }
      await Promise.all([...inFlight]);
    },

    async destroy(): Promise<void> {
      // Intake stops BEFORE the flush: during flush's await the handlers would
      // otherwise still be subscribed, and an update event landing there
      // (plugin teardown, not a user edit) would arm a fresh debounce timer
      // that nothing clears — firing its write after PdfDeck has closed the
      // document. flush() itself doesn't check `stopped`, so the pending
      // edits still go out.
      stopped = true;
      for (const unsubscribe of unsubscribes.splice(0)) unsubscribe();
      await this.flush();
      // No event can have armed a timer since `stopped` was set; anything
      // left here would fire against a closed document, so drop it.
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
    },
  };
}
