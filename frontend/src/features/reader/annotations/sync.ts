/// Keeps one open document's marks and the SQLite sidecar in step.
///
/// Ownership is the load-bearing idea. A PDF can arrive with annotations
/// already baked in by some other reader, and the plugin surfaces those exactly
/// like ours. If the save loop adopted them, the first save would copy them into
/// the sidecar, the next load would import them *on top of* the originals, and
/// the reader would show every one of them twice. So this tracks which ids are
/// ours — the rows we imported, plus the marks the user drew this session — and
/// persists nothing else. Foreign marks stay visible but read-only in effect:
/// `autoCommit: false` means an edit to one was never going to reach the PDF
/// anyway, so nothing is lost that was ever going to be kept.

import type { AnnotationEvent, AnnotationTransferItem } from '@embedpdf/plugin-annotation';
import type { PdfAnnotationObject } from '@embedpdf/models';
import { fromWire, kindOf, sameAsStored, toWire } from './adapter';
import {
  annotations,
  annotationList,
  isLoaded,
  loadAnnotations,
  removeAnnotation,
  saveAnnotation,
} from './store.svelte';

/// The slice of the plugin's document scope this needs. Narrow on purpose: it
/// is what lets the loop be tested without a PDF engine.
export interface SyncScope {
  importAnnotations(items: AnnotationTransferItem[]): void;
  getAnnotationById(id: string): { object: PdfAnnotationObject } | null;
}

export interface AnnotationSyncOptions {
  paperId: string;
  documentId: string;
  scope: SyncScope;
  /// `capability.onAnnotationEvent` — returns its own unsubscribe.
  subscribe: (handler: (e: AnnotationEvent) => void) => () => void;
  /// Called when a write fails, so the reader can say a mark did not save
  /// rather than losing it silently.
  onError?: (message: string) => void;
  debounceMs?: number;
}

export interface AnnotationSync {
  /// Load the sidecar and replay it into the document. Resolves once the marks
  /// have been handed to the plugin.
  start(): Promise<void>;
  /// Write anything still pending. Resolves when the writes settle.
  flush(): Promise<void>;
  /// Flush, then stop listening. Closing a tab must not drop the last edit.
  destroy(): Promise<void>;
}

const DEFAULT_DEBOUNCE_MS = 500;

export function createAnnotationSync(opts: AnnotationSyncOptions): AnnotationSync {
  const { paperId, documentId, scope, subscribe } = opts;
  const wait = opts.debounceMs ?? DEFAULT_DEBOUNCE_MS;

  const owned = new Set<string>();
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
    opts.onError?.((e as Error).message);
  }

  /// One mark's writes, in order. Undo and redo turn a create into a delete and
  /// back within a click or two, and two requests racing on the same id could
  /// land the PUT before the DELETE — leaving the reader looking at a mark the
  /// sidecar has forgotten. Queued work reads the plugin and the cache when it
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
      const tracked = scope.getAnnotationById(id);
      // Gone between the edit and the write — the delete path already ran.
      if (!tracked) return;
      const body = toWire({ annotation: tracked.object });
      // Defense in depth: ownership should already have excluded this.
      if (!body) return;
      const stored = annotations.byPaper[paperId]?.[id];
      // A move and a recolor both show up in the payload, so an unchanged mark
      // really is unchanged — worth checking, since selecting a mark can emit
      // an update that changes nothing.
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

  function onEvent(e: AnnotationEvent): void {
    if (e.documentId !== documentId || stopped) return;
    if (e.type === 'loaded') return; // the PDF's own marks; not ours to store
    const id = e.annotation.id;
    if (e.type === 'create') {
      // The subtype whitelist is what keeps a tool we never surfaced — or a
      // reply, a popup, a widget — out of the sidecar.
      if (!kindOf(e.annotation)) return;
      owned.add(id);
      // Written straight away rather than debounced: a mark is final the
      // moment it is drawn, and the sooner it is durable the better.
      write(id);
      return;
    }
    if (!owned.has(id)) return;
    if (e.type === 'delete') {
      owned.delete(id);
      cancel(id);
      enqueue(id, () => removeAnnotation(paperId, id));
      return;
    }
    schedule(id);
  }

  let unsubscribe: (() => void) | null = null;

  return {
    async start(): Promise<void> {
      unsubscribe = subscribe(onEvent);
      await loadAnnotations(paperId);
      if (stopped) return;
      // A failed load leaves this paper's marks UNKNOWN, which is not the
      // same as none (the store's `loaded` contract): skip the import — its
      // empty list would claim there is nothing — while the subscription
      // above keeps marks drawn THIS session saving normally.
      if (!isLoaded(paperId)) return;
      const items: AnnotationTransferItem[] = [];
      for (const row of annotationList(paperId)) {
        const item = fromWire(row);
        // A row whose payload could not be rebuilt is not handed to the plugin,
        // so the plugin can never raise an event about it — nothing to own. It
        // still shows in the annotations panel, which reads the store directly.
        if (!item) continue;
        owned.add(row.id);
        items.push(item);
      }
      // The plugin queues this until the PDF's own annotations have loaded, so
      // there is no race with the document opening.
      scope.importAnnotations(items);
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
      // Intake stops BEFORE the flush: during flush's await the handler would
      // otherwise still be subscribed, and an update event landing there
      // (plugin teardown, not a user edit) would arm a fresh debounce timer
      // that nothing clears — firing its write after PdfDeck has closed the
      // document. flush() itself doesn't check `stopped`, so the pending
      // edits still go out.
      stopped = true;
      unsubscribe?.();
      unsubscribe = null;
      await this.flush();
      // No event can have armed a timer since `stopped` was set; anything
      // left here would fire against a closed document, so drop it.
      for (const timer of timers.values()) clearTimeout(timer);
      timers.clear();
    },
  };
}
