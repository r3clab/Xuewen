import { identifyPaper, identifySearch } from './api';
import { classifyIdentifyInput } from './input';
import { identifyState, resetIdentifyFields } from './state.svelte';
import { loadPapers, loadStats } from '$features/library/actions';
import { markLibraryChanged, setDetail } from '$features/library/state.svelte';
import type { IdentifyBody } from '$shared/api/generated/IdentifyBody';
import type { PaperDetail } from '$shared/api/generated/PaperDetail';

// Superseded-session guard (same pattern as importSession): an in-flight
// search/apply from a closed or reopened modal must not write into the
// current session's identifyState.
let identifySession = 0;

export function openIdentify(
  paperId: string,
  current?: { doi: string | null; arxiv_id: string | null },
): void {
  identifySession++;
  resetIdentifyFields();
  identifyState.open = true;
  identifyState.paperId = paperId;
  identifyState.current = current ?? null;
}

export function closeIdentify(): void {
  identifySession++;
  resetIdentifyFields();
  identifyState.open = false;
  identifyState.paperId = null;
}

/// Search: title inputs hit /api/identify/search; DOI/arXiv inputs stage a
/// direct apply body (the backend fetches the authoritative record on apply).
export async function runIdentifySearch(): Promise<void> {
  const session = identifySession;
  const { kind, value } = classifyIdentifyInput(identifyState.input);
  identifyState.candidates = [];
  identifyState.selected = null;
  identifyState.direct = null;
  identifyState.error = null;
  if (!value) return;
  identifyState.busy = true;
  try {
    if (kind === 'title') {
      const cands = await identifySearch(value);
      if (session !== identifySession) return; // modal closed/reopened mid-flight
      identifyState.candidates = cands;
      if (!cands.length) identifyState.error = 'no candidates found';
    } else {
      identifyState.direct = kind === 'doi' ? { doi: value } : { arxiv_id: value };
    }
  } catch (e) {
    if (session !== identifySession) return;
    identifyState.error = (e as Error).message;
  } finally {
    if (session === identifySession) identifyState.busy = false;
  }
}

/// Apply the selected candidate (or the staged direct identifier).
export async function applyIdentify(onApplied: (id: string, d: PaperDetail) => void): Promise<void> {
  const session = identifySession;
  const id = identifyState.paperId;
  if (!id) return;
  const body: IdentifyBody | null = identifyState.selected
    ? { candidate: identifyState.selected }
    : identifyState.direct;
  if (!body) return;
  identifyState.busy = true;
  identifyState.error = null;
  try {
    const detail = await identifyPaper(id, body);
    // The server applied the match: refresh caches and lists regardless of
    // whether this identify session is still the live one...
    setDetail(id, detail);
    markLibraryChanged(); // identify can change the paper's title
    onApplied(id, detail);
    if (session === identifySession) {
      // ...but only the live session may close the modal.
      identifyState.open = false;
      identifyState.paperId = null;
    }
    await loadPapers();
    await loadStats();
  } catch (e) {
    if (session !== identifySession) return;
    identifyState.error = (e as Error).message;
  } finally {
    if (session === identifySession) identifyState.busy = false;
  }
}
