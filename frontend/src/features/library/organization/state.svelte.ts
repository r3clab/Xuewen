import type { Project } from '$shared/api/generated/Project';
import type { TagSummary } from '$shared/api/generated/TagSummary';

export const projects = $state<{ items: Project[] }>({ items: [] });

export const tags = $state<{ items: TagSummary[] }>({ items: [] });
