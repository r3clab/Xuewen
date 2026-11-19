import { request } from '$shared/api/http';
import type { CitationMatches } from '$shared/api/generated/CitationMatches';
import type { CitedReference } from '$shared/api/generated/CitedReference';
import type { MatchCitationsBody } from '$shared/api/generated/MatchCitationsBody';
import type { PaperSummary } from '$shared/api/generated/PaperSummary';
import type { ParseCitationsBody } from '$shared/api/generated/ParseCitationsBody';
import type { ParsedCitations } from '$shared/api/generated/ParsedCitations';
import type { StructuredReference } from '$shared/api/generated/StructuredReference';

/** Structured fields for extracted reference strings, index-aligned; null
 *  where the server's parsers produced nothing. */
export async function parseCitations(
  paperId: string,
  references: string[],
): Promise<(StructuredReference | null)[]> {
  const parsed = await request<ParsedCitations>(
    `/api/papers/${encodeURIComponent(paperId)}/citations`,
    'parsing citations failed',
    { method: 'POST', json: { references } satisfies ParseCitationsBody },
  );
  return parsed.references;
}

/** The library paper each reference cites, index-aligned; null where none. */
export async function matchCitations(references: CitedReference[]): Promise<(PaperSummary | null)[]> {
  const { matches } = await request<CitationMatches>('/api/citations/match', 'matching citations failed', {
    method: 'POST',
    json: { references } satisfies MatchCitationsBody,
  });
  return matches;
}
