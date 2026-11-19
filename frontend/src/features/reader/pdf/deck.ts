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

/** Split the documents that need opening into the one the user is actually
 *  looking at and the rest, which the caller defers.
 *
 *  Opening every restored tab at once is what made a reload crawl. The engine
 *  sends every document's work to one worker, one job at a time, and an open
 *  cannot be stopped part way once it is sent — so each background tab's parse
 *  that gets in line ahead of the visible tab's page renders holds them up, and
 *  a session restored with four tabs paints nothing until all four are parsed.
 *  Deferring is not "don't load background tabs" — they still load, just
 *  behind the one on screen, so switching to them stays instant. */
export function planOpens(
  toOpen: string[],
  activeId: string | null,
): { now: string[]; deferred: string[] } {
  // Nothing is on screen when there's no active tab, so nothing earns priority.
  const now = activeId !== null && toOpen.includes(activeId) ? [activeId] : [];
  const deferred = toOpen.filter((id) => id !== activeId);
  return { now, deferred };
}
