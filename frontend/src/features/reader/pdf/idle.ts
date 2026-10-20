/** Schedule `fn` once when the browser is idle (or after `timeout` ms,
 *  whichever comes first). Returns a cancel function. Falls back to a short
 *  setTimeout where requestIdleCallback is missing (jsdom, Safari). */
export function runWhenIdle(fn: () => void, timeout = 2000): () => void {
  if (typeof requestIdleCallback === 'function') {
    const id = requestIdleCallback(() => fn(), { timeout });
    return () => cancelIdleCallback(id);
  }
  const id = setTimeout(fn, 50);
  return () => clearTimeout(id);
}
