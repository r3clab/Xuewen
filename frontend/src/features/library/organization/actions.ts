import {
  addPaperToProject,
  addTag,
  createProject,
  deleteProject,
  listProjects,
  listTags,
  removePaperFromProject,
  removeTag,
  renameTag as apiRenameTag,
  deleteTag as apiDeleteTag,
  updateProject,
} from './api';
import { projects, tags } from './state.svelte';
import { loadPapers } from '$features/library/actions';
import { applyQueryEdit } from '$features/library/search/actions';
import { setQualifier } from '$features/library/search/query';
import { filters } from '$features/library/search/state.svelte';
import {
  cachedDetail,
  clearDetails,
  evictDetail,
  library,
  patchDetail,
} from '$features/library/state.svelte';
import type { Project } from '$shared/api/generated/Project';

export async function loadProjects(): Promise<void> {
  try {
    projects.items = await listProjects();
  } catch (e) {
    console.error(e);
  }
}

export async function loadTags(): Promise<void> {
  try {
    tags.items = await listTags();
  } catch (e) {
    console.error(e);
  }
}

export async function createNewProject(name: string): Promise<Project> {
  const p = await createProject(name);
  await loadProjects();
  return p;
}

export async function renameProject(id: string, patch: { name?: string }): Promise<void> {
  const wasActive = filters.project === id;
  await updateProject(id, patch);
  clearDetails();
  await loadProjects();
  // The `project:` qualifier is name-keyed, so an active filter must follow
  // the rename in the search string (the single source of truth) — the old
  // name would stop resolving to an id and silently match nothing on the
  // next sync. loadProjects ran first so the new name resolves.
  if (wasActive && patch.name) await applyQueryEdit(setQualifier(filters.q, 'project', patch.name));
  else await loadPapers();
}

export async function removeProject(id: string): Promise<void> {
  const wasActive = filters.project === id;
  await deleteProject(id);
  clearDetails();
  await loadProjects();
  // Drop the dead qualifier from the search string itself — clearing only the
  // cached filters.project would leave `project:` in the box to resurrect the
  // filter on the next sync.
  if (wasActive) await applyQueryEdit(setQualifier(filters.q, 'project', null));
  else await loadPapers();
}

export async function addToProject(paperId: string, projectId: string): Promise<void> {
  await addPapersToProject([paperId], projectId);
}

/// Bulk variant: the memberships are added in parallel and the project list /
/// paper list refresh happens ONCE — per-paper `addToProject` calls would
/// refetch them once per paper. A partial failure still refreshes for the
/// memberships that landed, then rejects naming the failed count, so the
/// caller's error surface tells the truth about what happened.
export async function addPapersToProject(paperIds: string[], projectId: string): Promise<void> {
  const results = await Promise.allSettled(paperIds.map((id) => addPaperToProject(id, projectId)));
  const added = paperIds.filter((_, i) => results[i].status === 'fulfilled');
  for (const id of added) evictDetail(id);
  if (added.length > 0) {
    await loadProjects();
    if (filters.project === projectId) await loadPapers();
  }
  const failures = results.filter((r): r is PromiseRejectedResult => r.status === 'rejected');
  if (failures.length > 0) {
    if (paperIds.length === 1) throw failures[0].reason;
    throw new Error(
      `couldn't add ${failures.length} of ${paperIds.length} papers: ${(failures[0].reason as Error).message}`,
    );
  }
}

export async function removeFromProject(paperId: string, projectId: string): Promise<void> {
  await removePaperFromProject(paperId, projectId);
  evictDetail(paperId);
  await loadProjects();
  if (filters.project === projectId) await loadPapers();
}

/// Add a tag (by name; creating it if new) to a paper, patch the row/cached
/// detail, and refresh the tags store (name list + counts).
export async function addTagToPaper(paperId: string, name: string): Promise<void> {
  await addTagToPapers([paperId], name);
}

/// Bulk variant: rows are patched per paper but the tags store / paper list
/// refresh happens ONCE at the end — per-paper `addTagToPaper` calls would
/// refetch them once per paper. The adds stay sequential (not Promise.all):
/// a new tag name is created by whichever add lands first, and racing that
/// creation across requests is not worth the latency win.
export async function addTagToPapers(paperIds: string[], name: string): Promise<void> {
  for (const paperId of paperIds) {
    const tag = await addTag(paperId, name);
    const row = library.papers.find((p) => p.id === paperId);
    if (row && !row.tags.some((t) => t.id === tag.id)) row.tags = [...row.tags, tag];
    const cached = cachedDetail(paperId);
    if (cached && !cached.tags.some((t) => t.id === tag.id))
      patchDetail(paperId, { tags: [...cached.tags, tag] });
  }
  await loadTags();
  if (filters.tag) await loadPapers();
}

export async function removeTagFromPaper(paperId: string, tagId: string): Promise<void> {
  await removeTag(paperId, tagId);
  const row = library.papers.find((p) => p.id === paperId);
  if (row) row.tags = row.tags.filter((t) => t.id !== tagId);
  const cached = cachedDetail(paperId);
  if (cached) patchDetail(paperId, { tags: cached.tags.filter((t) => t.id !== tagId) });
  await loadTags();
  if (filters.tag) await loadPapers();
}

/// Rename a tag globally (not per-paper): refresh the tags store and reload
/// the paper list so row chips pick up the new name. The `tag:` qualifier is
/// name-keyed, so if the renamed tag was the active filter the search string
/// is rewritten and the filter follows the rename — to the server's echo, not
/// the typed name: rename normalizes ('nlp / eval' is stored as 'nlp/eval'),
/// and the tag filter is an exact name match, so the raw string would leave a
/// qualifier that matches nothing.
export async function renameTag(id: string, name: string): Promise<void> {
  const tag = tags.items.find((t) => t.id === id);
  const stored = await apiRenameTag(id, name);
  clearDetails();
  await loadTags();
  if (tag && filters.tag === tag.name)
    await applyQueryEdit(setQualifier(filters.q, 'tag', stored.name));
  else await loadPapers();
}

/// Delete a tag from every paper carrying it (GC'd tag row included), then
/// refresh the tags store and paper list. If it was the active filter, the
/// dead qualifier is dropped from the search string itself — clearing only
/// the cached filters.tag would leave `tag:` in the box to resurrect an
/// always-empty filter on the next sync.
export async function deleteTag(id: string): Promise<void> {
  const tag = tags.items.find((t) => t.id === id);
  await apiDeleteTag(id);
  clearDetails();
  await loadTags();
  if (tag && filters.tag === tag.name) await applyQueryEdit(setQualifier(filters.q, 'tag', null));
  else await loadPapers();
}
