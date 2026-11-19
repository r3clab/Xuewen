import { request } from '$shared/api/http';
import type { Engine } from './types';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';
import type { SearchResponse } from '$shared/api/generated/SearchResponse';
import type { SearchStatus } from '$shared/api/generated/SearchStatus';

/// Query string for /api/search. The raw query string carries every filter
/// (tag:/project:/is:/status:/in:/author: qualifiers are parsed server-side);
/// only the engine selection travels as a separate param, omitted when both
/// engines are on (the server default) so URLs stay short and cacheable.
export function searchParams(q: string, engines: Record<Engine, boolean>, keywordOnly = false): URLSearchParams {
  const params = new URLSearchParams();
  params.set('q', q);
  const selected = keywordOnly ? ['keyword'] : (['keyword', 'semantic'] as const).filter((k) => engines[k]);
  if (selected.length > 0 && selected.length < 2) params.set('engines', selected.join(','));
  return params;
}

export async function searchPapers(
  q: string,
  engines: Record<Engine, boolean>,
  keywordOnly = false,
): Promise<SearchResponse<PaperSummary>> {
  return request(`/api/search?${searchParams(q, engines, keywordOnly).toString()}`, 'search failed');
}

export async function getSearchStatus(): Promise<SearchStatus> {
  return request('/api/search/status', 'search status failed');
}
