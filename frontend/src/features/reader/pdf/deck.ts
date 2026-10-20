import type { PdfDocumentObject } from '@embedpdf/models';

/** Given the docs currently opened and the current tab ids, decide which to
 *  open and which to close. Pure — the caller performs the side effects. */
export function reconcileDocuments(
  opened: Iterable<string>,
  tabIds: string[],
): { toOpen: string[]; toClose: string[] } {
  const openedSet = new Set(opened);
  const tabSet = new Set(tabIds);
  const toOpen = tabIds.filter((id) => !openedSet.has(id));
  const toClose = [...openedSet].filter((id) => !tabSet.has(id));
  return { toOpen, toClose };
}

/** Documents the registry already holds that the deck's own bookkeeping missed,
 *  for the caller to fold into `opened` before reconciling.
 *
 *  PdfDeck is NOT mounted once, despite reading like it is. `<EmbedPDF>` renders
 *  its children from two different branches (@embedpdf/core 2.14.4): bare while
 *  the registry initializes, then — the instant `pluginsReady` flips — again
 *  inside the AutoMount wrapper, which is where the annotation plugin's
 *  `RendererRegistryProvider` comes from, so it can't simply be turned off with
 *  `autoMountDomElements={false}`. That branch swap destroys and recreates the
 *  whole subtree, handing the new PdfDeck a fresh, EMPTY `opened` set. It then
 *  opened the active paper a second time, and the second load's `setAnnotations`
 *  replaced the annotation plugin's document map wholesale — wiping every mark
 *  the first load had already imported from the sidecar, which is why saved
 *  annotations came back for about a second and then vanished. The registry
 *  survives the remount, so it, not a component-local Set, is the authority on
 *  what is already open; `documentOrder` carries an id from the moment loading
 *  starts, so an open still in flight counts too.
 *
 *  Only ids that belong to a tab are adopted. The registry also holds the
 *  throwaway `export:<paper id>` document while an annotated PDF is being built
 *  (annotations/export.ts); adopting that would put it in `opened`, where the
 *  very next reconcile would see no tab for it and close it mid-export. */
export function documentsToAdopt(
  opened: Iterable<string>,
  tabIds: string[],
  registryIds: string[],
): string[] {
  const openedSet = new Set(opened);
  const tabSet = new Set(tabIds);
  return registryIds.filter((id) => tabSet.has(id) && !openedSet.has(id));
}

/** Split the documents that need opening into the one the user is actually
 *  looking at and the rest, which the caller defers.
 *
 *  Opening every restored tab at once is what made a reload crawl. EmbedPDF
 *  runs document opens through a single engine lane (the task queue is
 *  concurrency 1) at CRITICAL priority — above renderPage — and a started task
 *  is never preempted. So each background tab's parse lands *ahead* of the
 *  visible tab's page rasters, and a session restored with four tabs paints
 *  nothing until all four have been parsed and their pages rasterized: the
 *  renderer stayed jammed for tens of seconds. Deferring is not "don't load
 *  background tabs" — they still load, just behind the one on screen, so
 *  switching to them stays instant. */
export function planOpens(
  toOpen: string[],
  activeId: string | null,
): { now: string[]; deferred: string[] } {
  // Nothing is on screen when there's no active tab, so nothing earns priority.
  const now = activeId !== null && toOpen.includes(activeId) ? [activeId] : [];
  const deferred = toOpen.filter((id) => id !== activeId);
  return { now, deferred };
}

/** Which half of `openDocumentFully` failed. Callers need the distinction:
 *  PdfDeck rolls back an 'open' failure so the next effect run retries (the
 *  maxDocuments cap frees up when a tab closes) but NOT a 'load' one — a
 *  broken PDF must not be reopened on every tab change. */
export type DocumentOpenPhase = 'open' | 'load';

export class DocumentOpenError extends Error {
  constructor(
    /** 'open': the outer task rejected — the manager's cap was hit or the
     *  document errored before an id was assigned. 'load': the id exists but
     *  reading/parsing the document failed. */
    readonly phase: DocumentOpenPhase,
    cause: unknown,
  ) {
    // Engine/plugin failures arrive as PdfErrorReason — a plain { code,
    // message }, not an Error — so the message is dug out rather than read
    // off `.message` and hoped for (same rule as exportErrorMessage).
    const message = (cause as { message?: unknown } | null)?.message;
    super(typeof message === 'string' && message !== '' ? message : `document ${phase} failed`);
    this.name = 'DocumentOpenError';
  }
}

/** The slice of the document-manager capability `openDocumentFully` needs,
 *  declared structurally so the flow is testable without a PDF engine (the
 *  same reason pdfCopy.ts declares SelectionLike). */
export interface DocumentOpenerLike {
  openDocumentUrl(opts: { url: string; documentId: string; autoActivate: boolean }): {
    toPromise(): Promise<{ task: { toPromise(): Promise<PdfDocumentObject> } }>;
  };
}

/** Open a document and resolve only once it is actually LOADED.
 *
 *  `openDocumentUrl`'s outer task resolves SYNCHRONOUSLY, carrying the real
 *  load task nested inside it (plugin-document-manager's openDocumentUrl:169),
 *  so waiting on the outer one reports "loaded" before a single byte has been
 *  read. Every caller must chain on the inner task; this helper is the one
 *  place that knows that — a caller that forgets gets the bug back silently
 *  (PdfDeck's deferred background opens would all start at once, and an
 *  export would commit into a half-read document). Failures reject with a
 *  DocumentOpenError so the two phases stay distinguishable (see above). */
export async function openDocumentFully(
  cap: DocumentOpenerLike,
  opts: { url: string; documentId: string },
): Promise<PdfDocumentObject> {
  let opened: { task: { toPromise(): Promise<PdfDocumentObject> } };
  try {
    opened = await cap.openDocumentUrl({ ...opts, autoActivate: false }).toPromise();
  } catch (e) {
    throw new DocumentOpenError('open', e);
  }
  try {
    return await opened.task.toPromise();
  } catch (e) {
    throw new DocumentOpenError('load', e);
  }
}
