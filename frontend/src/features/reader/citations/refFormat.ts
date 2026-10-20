import { titleCase } from 'title-case';
import type { StructuredReference } from '$shared/api/generated/StructuredReference';

/** Display title case for reference titles that arrive in sentence case.
 *  `title-case` preserves intentional casing (PGFUZZ, GPT-4, eBPF, iOS) and
 *  keeps small words lowercase except first/last or after a colon. */
export { titleCase };

/** One or two authors verbatim; three or more collapse to first-…-last. */
export function authorLine(authors: string[]): string {
  if (authors.length <= 2) return authors.join(', ');
  return `${authors[0]}, …, ${authors[authors.length - 1]}`;
}

/** External links for a reference: structured DOI/arXiv/URL first, then the
 *  raw in-PDF link, deduped by href, at most two. */
export function refLinks(
  s: StructuredReference | null | undefined,
  externalUrl?: string,
): { label: string; href: string }[] {
  const out: { label: string; href: string }[] = [];
  const push = (label: string, href: string) => {
    // Only web URLs may become links: `url` comes from LLM output (prompt
    // contains raw PDF text) and `externalUrl` from raw PDF /URI actions —
    // a javascript:/data: href here would run in the app origin.
    try {
      const proto = new URL(href).protocol;
      if (proto !== 'http:' && proto !== 'https:') return;
    } catch {
      return; // unparseable → no link
    }
    if (!out.some((l) => l.href === href)) out.push({ label, href });
  };
  if (s?.doi) push('doi.org', `https://doi.org/${s.doi}`);
  if (s?.arxiv_id) push('arXiv', `https://arxiv.org/abs/${s.arxiv_id}`);
  if (s?.url) push(hostOf(s.url), s.url);
  if (externalUrl) push(hostOf(externalUrl), externalUrl);
  return out.slice(0, 2);
}

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}
