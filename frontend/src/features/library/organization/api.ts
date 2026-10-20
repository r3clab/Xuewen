import { request, requestVoid } from '$shared/api/http';
import type { Project } from '$shared/api/generated/Project';
import type { Tag } from '$shared/api/generated/Tag';
import type { TagSummary } from '$shared/api/generated/TagSummary';

export async function listProjects(): Promise<Project[]> {
  return request('/api/projects', 'projects failed');
}

export async function createProject(name: string): Promise<Project> {
  return request('/api/projects', 'create project failed', { method: 'POST', json: { name } });
}

export async function updateProject(id: string, patch: { name?: string }): Promise<Project> {
  return request(`/api/projects/${encodeURIComponent(id)}`, 'update project failed', {
    method: 'PATCH',
    json: patch,
  });
}

export async function deleteProject(id: string): Promise<void> {
  return requestVoid(`/api/projects/${encodeURIComponent(id)}`, 'delete project failed', {
    method: 'DELETE',
  });
}

export async function addPaperToProject(paperId: string, projectId: string): Promise<void> {
  return requestVoid(
    `/api/papers/${encodeURIComponent(paperId)}/projects/${encodeURIComponent(projectId)}`,
    'add to project failed',
    { method: 'PUT' },
  );
}

export async function removePaperFromProject(paperId: string, projectId: string): Promise<void> {
  return requestVoid(
    `/api/papers/${encodeURIComponent(paperId)}/projects/${encodeURIComponent(projectId)}`,
    'remove from project failed',
    { method: 'DELETE' },
  );
}

export async function listTags(): Promise<TagSummary[]> {
  return request('/api/tags', 'tags failed');
}

export async function addTag(paperId: string, name: string): Promise<Tag> {
  return request(`/api/papers/${encodeURIComponent(paperId)}/tags`, 'add tag failed', {
    method: 'PUT',
    json: { name },
  });
}

export async function removeTag(paperId: string, tagId: string): Promise<void> {
  return requestVoid(
    `/api/papers/${encodeURIComponent(paperId)}/tags/${encodeURIComponent(tagId)}`,
    'remove tag failed',
    { method: 'DELETE' },
  );
}

/// Returns the tag as the server stored it. Rename normalizes the name
/// ('nlp / eval' → 'nlp/eval') — callers treat that echo as authoritative,
/// not their input.
export async function renameTag(id: string, name: string): Promise<Tag> {
  return request(`/api/tags/${encodeURIComponent(id)}`, 'rename tag failed', {
    method: 'PATCH',
    json: { name },
  });
}

export async function deleteTag(id: string): Promise<void> {
  return requestVoid(`/api/tags/${encodeURIComponent(id)}`, 'delete tag failed', {
    method: 'DELETE',
  });
}
