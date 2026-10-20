import { exportPaper } from './api';
import { bibFormat } from './state.svelte';
import type { BibFormat } from './types';
import { copyText } from '$shared/browser/clipboard';

/// Fetch a paper's citation and copy it to the clipboard. Defaults to the
/// current format setting; callers with a fixed-label button (e.g. the
/// context menu's "Copy BibTeX") pass an explicit format instead.
export async function copyCitation(id: string, format: BibFormat = bibFormat.value): Promise<void> {
  const text = await exportPaper(id, format);
  await copyText(text);
}
