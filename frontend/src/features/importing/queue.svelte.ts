import { importPaper, importUrl } from './api';
import { loadPapers, loadStats } from '$features/library/actions';
import { markLibraryChanged } from '$features/library/state.svelte';

export interface ImportItem {
  name: string;
  status:
    | 'queued'
    | 'importing'
    | 'ingested'
    | 'duplicate'
    | 'same-work'
    | 'in-trash'
    | 'unfetched'
    | 'failed';
  message?: string;
  needsReview?: boolean;
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
    const index = importState.items.push({ name: file.name, status: 'queued' }) - 1;
    pending.push({ job: { kind: 'file', file }, index, session });
  }
  return startDrain();
}

/// Queue a URL/identifier for import and (re)start the sequential drain.
/// Resolves when the current batch finishes.
export function enqueueUrl(input: string): Promise<void> {
  const session = importSession;
  const index = importState.items.push({ name: input, status: 'queued' }) - 1;
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
    importState.items[item.index].status = 'importing';
    try {
      const res =
        item.job.kind === 'file' ? await importPaper(item.job.file) : await importUrl(item.job.input);
      if (item.session !== importSession) continue; // a new session started mid-fetch
      if (res.outcome === 'duplicate') {
        importState.items[item.index].status = 'duplicate';
      } else if (res.outcome === 'same_work') {
        importState.items[item.index].status = 'same-work';
      } else if (res.outcome === 'in_trash') {
        importState.items[item.index].status = 'in-trash';
        importState.items[item.index].message = res.id;
      } else if (res.outcome === 'unfetched') {
        importState.items[item.index].status = 'unfetched';
        importState.items[item.index].message = res.title ?? '(untitled)';
      } else {
        importState.items[item.index].status = 'ingested';
        importState.items[item.index].message = res.title ?? '(untitled)';
        importState.items[item.index].needsReview = res.status === 'needs_review';
      }
    } catch (e) {
      if (item.session !== importSession) continue;
      importState.items[item.index].status = 'failed';
      importState.items[item.index].message = (e as Error).message;
    }
  }
  // Reflect the newly ingested papers in the library table and counts.
  markLibraryChanged();
  await loadPapers();
  await loadStats();
}
