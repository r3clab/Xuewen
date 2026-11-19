import { exportPaper } from './api';
import { bibFormat } from './state.svelte';
import { copyText } from '$shared/browser/clipboard';
import type { BibFormat } from '$shared/api/generated/BibFormat';

/// Copy a paper's citation to the clipboard. Defaults to the current format
/// setting; callers with a fixed-label button (e.g. the context menu's "Copy
/// BibTeX") pass an explicit format instead.
///
/// The clipboard write starts while the entry is still being fetched: WebKit
/// (the desktop app is a WKWebView) only lets a write run inside the click
/// that asked for it, and awaiting the fetch first would leave that gesture.
/// A ClipboardItem takes the pending text and waits for it itself. Without
/// one (an insecure context has no async Clipboard API) the text is awaited
/// and copied the legacy way.
export async function copyCitation(id: string, format: BibFormat = bibFormat.value): Promise<void> {
  const text = exportPaper(id, format);
  if (typeof ClipboardItem === 'undefined' || !navigator.clipboard?.write) {
    await copyText(await text);
    return;
  }
  const item = new ClipboardItem({ 'text/plain': text.then((t) => new Blob([t], { type: 'text/plain' })) });
  try {
    await navigator.clipboard.write([item]);
  } catch (e) {
    // A failed fetch, not the write it starved, is the error to report.
    await text;
    throw e;
  }
}
