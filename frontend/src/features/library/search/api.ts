import { request } from '$shared/api/http';
import type { Filters, SearchOpts } from './types';
import type { SearchResponse } from '$shared/api/generated/SearchResponse';
import type { SearchStatus } from '$shared/api/generated/SearchStatus';

/** The list-filter query params shared by /api/papers and /api/papers/export. */
export function filterParams(f: Filters): URLSearchParams {
  const params = new URLSearchParams();
  if (f.q.trim()) params.set('q', f.q.trim());
  if (f.status !== 'all') params.set('status', f.status);
  params.set('sort', f.sort);
  if (f.project && f.project !== 'all') params.set('project', f.project);
  if (f.tag) params.set('tag', f.tag);
  if (f.starred) params.set('starred', 'true');
  return params;
}

/// Query string for /api/search. The raw query string carries every filter
/// (tag:/project:/is:/status:/in:/author: qualifiers are parsed server-side);
/// only the engine selection travels as a separate param, omitted when both
/// engines are on (the server default) so URLs stay short and cacheable.
export function searchParams(q: string, opts: SearchOpts, keywordOnly = false): URLSearchParams {
  const params = new URLSearchParams();
  params.set('q', q);
  const engines = keywordOnly
    ? ['keyword']
    : (['keyword', 'semantic'] as const).filter((k) => opts[k]);
  if (engines.length > 0 && engines.length < 2) params.set('engines', engines.join(','));
  return params;
}

export async function searchPapers(
  q: string,
  opts: SearchOpts,
  keywordOnly = false,
): Promise<SearchResponse> {
  return request(`/api/search?${searchParams(q, opts, keywordOnly).toString()}`, 'search failed');
}

export async function getSearchStatus(): Promise<SearchStatus> {
  return request('/api/search/status', 'search status failed');
}
