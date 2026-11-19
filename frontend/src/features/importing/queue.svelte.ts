import { importPaper, importUrl } from './api';
import { loadPapers, loadStats } from '$features/library/actions';
import { restorePaper } from '$features/library/api';
import { errorMessage } from '$shared/errors';
import { toast } from '$shared/ui/toasts.svelte';
import type { ImportResult } from '$shared/api/generated/ImportResult';

/// A row's progress: the server's own outcome once it answers, plus the
/// client-side phases around it.
export type ImportState =
  | { outcome: 'queued' }
  | { outcome: 'importing' }
  | { outcome: 'failed'; message: string }
  | { outcome: 'restored' }
  | ImportResult;

export interface ImportItem {
  name: string;
  state: ImportState;
}

export const importState = $state<{ items: ImportItem[]; cancelled: boolean; open: boolean }>({
  items: [],
  cancelled: false,
  open: false,
});

// Work waiting to be imported (an uploaded file or a URL/identifier string),
// paired with its row index in importState.items and the import session it
// belongs to.
type Job = { kind: 'file'; file: File } | { kind: 'url'; input: string };
const pending: { job: Job; index: number; session: number }[] = [];
let draining: Promise<void> | null = null;
let importSession = 0;

export function openImport(): void {
  importSession++;
  pending.length = 0;
  importState.items = [];
  importState.cancelled = false;
  importState.open = true;
}

export function closeImport(): void {
  importState.cancelled = true;
  importState.open = false;
}

/// Queue files for import and (re)start the sequential drain. Resolves when the
/// current batch finishes.
export function enqueueFiles(files: File[]): Promise<void> {
  const session = importSession;
  for (const file of files) {
    const index = importState.items.push({ name: file.name, state: { outcome: 'queued' } }) - 1;
    pending.push({ job: { kind: 'file', file }, index, session });
  }
  return startDrain();
}

/// Queue a URL/identifier for import and (re)start the sequential drain.
/// Resolves when the current batch finishes.
export function enqueueUrl(input: string): Promise<void> {
  const session = importSession;
  const index = importState.items.push({ name: input, state: { outcome: 'queued' } }) - 1;
  pending.push({ job: { kind: 'url', input }, index, session });
  return startDrain();
}

function startDrain(): Promise<void> {
  if (!draining) {
    draining = drainQueue().finally(() => {
      draining = null;
    });
  }
  return draining;
}


async function drainQueue(): Promise<void> {
  while (pending.length > 0) {
    const item = pending.shift()!;
    // Skip work that was cancelled or belongs to a superseded import session.
    if (importState.cancelled || item.session !== importSession) continue;
    const row = importState.items[item.index];
    row.state = { outcome: 'importing' };
    try {
      const res =
        item.job.kind === 'file' ? await importPaper(item.job.file) : await importUrl(item.job.input);
      if (item.session !== importSession) continue; // a new session started mid-fetch
      row.state = res;
    } catch (e) {
      if (item.session !== importSession) continue;
      row.state = { outcome: 'failed', message: errorMessage(e, 'import failed') };
    }
  }
  await loadPapers();
  await loadStats();
}

/// Un-trash the paper an import found in the trash, then show it in the list.
export async function restoreTrashed(row: ImportItem): Promise<void> {
  if (row.state.outcome !== 'in_trash') return;
  try {
    await restorePaper(row.state.id);
    row.state = { outcome: 'restored' };
    await loadPapers();
    await loadStats();
  } catch (e) {
    toast('error', `Couldn't restore: ${errorMessage(e, 'restore failed')}`);
  }
}
