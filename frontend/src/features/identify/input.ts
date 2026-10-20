import type { Candidate } from '$shared/api/generated/Candidate';
import type { IdentifyBody } from '$shared/api/generated/IdentifyBody';

/// Whether applying the currently selected candidate would drop an
/// identifier (DOI/arXiv id) the paper currently has.
export function dropsIdentifier(s: {
  selected: Candidate | null;
  current: { doi: string | null; arxiv_id: string | null } | null;
}): boolean {
  if (!s.selected || !s.current) return false;
  return Boolean(
    (s.current.doi && !s.selected.doi) || (s.current.arxiv_id && !s.selected.arxiv_id),
  );
}

const DOI_RE = /10\.\d{4,9}\/\S+/;
const ARXIV_RE = /^\d{4}\.\d{4,5}(v\d+)?$/;
const ARXIV_URL_RE = /arxiv\.org\/(?:abs|pdf)\/(\d{4}\.\d{4,5}(?:v\d+)?)/i;

/// Warning for pseudo-DOIs that can never resolve. ACM Digital Library uses
/// the reserved 10.5555 prefix for papers it hosts WITHOUT a registered DOI
/// (typically USENIX/NDSS) — Crossref and doi.org have never heard of them.
export function pseudoDoiHint(direct: IdentifyBody | null): string | null {
  if (direct && 'doi' in direct && direct.doi.startsWith('10.5555/')) {
    return '10.5555/… is an ACM DL internal id, not a registered DOI — it will not resolve; try a title search instead.';
  }
  return null;
}

/// Classify what the user pasted: a DOI (even inside a doi.org URL), an arXiv
/// id (bare or inside an arxiv.org URL), or a title query.
export function classifyIdentifyInput(
  input: string,
): { kind: 'doi' | 'arxiv' | 'title'; value: string } {
  const t = input.trim();
  const doi = t.match(DOI_RE);
  // Strip punctuation that rides along when a DOI is copied out of prose.
  if (doi) return { kind: 'doi', value: doi[0].replace(/[.,;)\]}"']+$/, '') };
  const arxivUrl = t.match(ARXIV_URL_RE);
  if (arxivUrl) return { kind: 'arxiv', value: arxivUrl[1] };
  if (ARXIV_RE.test(t)) return { kind: 'arxiv', value: t };
  return { kind: 'title', value: t };
}
