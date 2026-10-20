import { applyIdentify as applyIdentifyWith } from '$features/identify/actions';
import { loadPapers, loadStats } from '$features/library/actions';
import { deletePaper, restorePaper, setPaperName as apiSetPaperName } from '$features/library/api';
import { filters } from '$features/library/search/state.svelte';
import { applyPaperName, dropPaper, markLibraryChanged } from '$features/library/state.svelte';
import { toast } from '$shared/ui/toasts.svelte';
import { closeTab, renameTab, retitleTab, selection } from '$features/reader/tabs/state.svelte';

/// Soft-delete one paper on the server and drop it from the UI: close its
/// tab, remove it from the list, forget its cached detail.
async function deleteAndDrop(id: string): Promise<void> {
  await deletePaper(id);
  closeTab(id);
  dropPaper(id);
  markLibraryChanged();
  if (selection.id === id) selection.id = null;
}

/// Delete a paper with an Undo toast (deletes are soft — POST /restore
/// un-trashes) on a longer timeout so there's time to reach for it.
export async function removePaper(id: string): Promise<void> {
  await removePapers([id]);
}

/// Bulk delete: every id is dropped, then ONE combined toast carries the
/// Undo for the whole batch — per-paper toasts would stack unusably.
/// Failures surface here too, as one combined error toast, so callers need
/// no catch of their own (this never rejects); the Undo restores only the
/// ids that were actually deleted.
export async function removePapers(ids: string[]): Promise<void> {
  const results = await Promise.allSettled(ids.map((id) => deleteAndDrop(id)));
  await loadStats();
  const deleted = ids.filter((_, i) => results[i].status === 'fulfilled');
  const failures = results.filter((r): r is PromiseRejectedResult => r.status === 'rejected');
  if (deleted.length > 0) {
    const message = deleted.length === 1 ? 'Paper deleted' : `${deleted.length} papers deleted`;
    toast('success', message, 8000, { label: 'Undo', run: () => void undoDelete(deleted) });
  }
  if (failures.length > 0) {
    const label =
      failures.length === 1 ? "Couldn't delete" : `Couldn't delete ${failures.length} papers`;
    toast('error', `${label}: ${(failures[0].reason as Error).message}`);
  }
}

async function undoDelete(ids: string[]): Promise<void> {
  try {
    await Promise.all(ids.map((id) => restorePaper(id)));
    markLibraryChanged();
    await loadPapers();
    await loadStats();
    toast('success', ids.length === 1 ? 'Paper restored' : `${ids.length} papers restored`);
  } catch (e) {
    toast('error', `Couldn't restore: ${(e as Error).message}`);
  }
}

/// Set (or clear) a paper's manual name. Await first, then patch the row/
/// cached detail from the server's echo (authoritative: it re-trims and
/// normalizes empty to null). Name is never a filter, so the list only
/// reloads when its order depends on it.
export async function setPaperName(paperId: string, name: string | null): Promise<void> {
  const { name: stored } = await apiSetPaperName(paperId, name);
  applyPaperName(paperId, stored);
  // The strip labels an open tab by its name, so a rename has to reach the tab
  // too — otherwise the label stays stale until the next reload.
  renameTab(paperId, stored);
  if (filters.sort === 'name') await loadPapers();
}

export function applyIdentify(): Promise<void> {
  return applyIdentifyWith((id, d) => retitleTab(id, d.title));
}
