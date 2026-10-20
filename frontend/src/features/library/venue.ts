// Display-only venue abbreviation. The full venue string is kept everywhere in
// the data/API — this only shapes what the UI shows. Resolution order:
//   1. curated rules (full name OR bare acronym → canonical form)
//   2. trailing parenthetical acronym  ("… (SP)" → "SP")
//   3. leading-year strip              ("2019 Foo" → "Foo")
//   4. unchanged
//
// Rules are checked in order; first match wins. To add a venue, add a line.
const RULES: { pattern: RegExp; abbr: string }[] = [
  // Security
  { pattern: /security and privacy|\bs\s?&\s?p\b|\boakland\b/i, abbr: 'S&P' },
  { pattern: /usenix security/i, abbr: 'SEC' },
  { pattern: /computer and communications security|\bccs\b/i, abbr: 'CCS' },
  { pattern: /network and distributed system security|\bndss\b/i, abbr: 'NDSS' },
  // ML / AI
  { pattern: /neural information processing systems|\bneur?ips\b/i, abbr: 'NeurIPS' },
  { pattern: /international conference on machine learning|\bicml\b/i, abbr: 'ICML' },
  { pattern: /international conference on learning representations|\biclr\b/i, abbr: 'ICLR' },
  { pattern: /association for the advancement of artificial intelligence|\baaai\b/i, abbr: 'AAAI' },
  // Anchored: the bare journal name is a substring of JAIR, AI Review, AIJ's own
  // special issues — matching loosely would collapse all of them into AIJ.
  { pattern: /^(?:artificial intelligence|artif\.?\s*intell\.?)$/i, abbr: 'AIJ' },
  { pattern: /computer vision and pattern recognition|\bcvpr\b/i, abbr: 'CVPR' },
  { pattern: /international conference on computer vision|\biccv\b/i, abbr: 'ICCV' },
  { pattern: /european conference on computer vision|\beccv\b/i, abbr: 'ECCV' },
  // NLP
  { pattern: /north american chapter of the association for computational linguistics|\bnaacl\b/i, abbr: 'NAACL' },
  { pattern: /empirical methods in natural language processing|\bemnlp\b/i, abbr: 'EMNLP' },
  { pattern: /association for computational linguistics|\bacl\b/i, abbr: 'ACL' },
  // Systems
  { pattern: /symposium on operating systems principles|\bsosp\b/i, abbr: 'SOSP' },
  { pattern: /operating systems design and implementation|\bosdi\b/i, abbr: 'OSDI' },
  { pattern: /usenix annual technical conference|usenix atc|\batc\b/i, abbr: 'USENIX ATC' },
  { pattern: /european conference on computer systems|\beurosys\b/i, abbr: 'EuroSys' },
  { pattern: /networked systems design and implementation|\bnsdi\b/i, abbr: 'NSDI' },
  { pattern: /file and storage technologies|\bfast\b/i, abbr: 'FAST' },
  // PL
  { pattern: /programming language design and implementation|\bpldi\b/i, abbr: 'PLDI' },
  { pattern: /principles of programming languages|\bpopl\b/i, abbr: 'POPL' },
  { pattern: /object-oriented programming.*systems.*languages|\boopsla\b/i, abbr: 'OOPSLA' },
  { pattern: /international conference on functional programming|\bicfp\b/i, abbr: 'ICFP' },
  // Last of the PL rules: PACMPL is the container, and its issue marker —
  // "Proc. ACM Program. Lang. 8(POPL)" — is the venue people mean, so the
  // four conference rules above have to get first refusal.
  { pattern: /proceedings of the acm on programming languages|\bpacmpl\b|proc\.?\s*acm\s*program\.?\s*lang\.?/i, abbr: 'PACMPL' },
  // Databases
  { pattern: /management of data|\bsigmod\b/i, abbr: 'SIGMOD' },
  { pattern: /very large data bases|\bvldb\b/i, abbr: 'VLDB' },
  { pattern: /international conference on data engineering|\bicde\b/i, abbr: 'ICDE' },
  // Theory
  { pattern: /symposium on theory of computing|\bstoc\b/i, abbr: 'STOC' },
  { pattern: /foundations of computer science|\bfocs\b/i, abbr: 'FOCS' },
  { pattern: /symposium on discrete algorithms|\bsoda\b/i, abbr: 'SODA' },
  // Networking / SE / HCI
  { pattern: /data communication|\bsigcomm\b/i, abbr: 'SIGCOMM' },
  { pattern: /international conference on software engineering|\bicse\b/i, abbr: 'ICSE' },
  { pattern: /foundations of software engineering|\bfse\b/i, abbr: 'FSE' },
  { pattern: /software testing and analysis|\bissta\b/i, abbr: 'ISSTA' },
  { pattern: /human factors in computing systems|\bchi\b/i, abbr: 'CHI' },
];

/** Abbreviate a venue for display. `null`/empty pass through unchanged. */
export function abbreviateVenue(venue: string | null): string | null {
  if (!venue) return venue;
  const v = venue.trim();
  if (!v) return venue;

  for (const { pattern, abbr } of RULES) {
    if (pattern.test(v)) return abbr;
  }

  // Fallback: a short trailing parenthetical is usually the acronym.
  const paren = v.match(/\(([^)]{1,20})\)\s*$/);
  if (paren) return paren[1];

  // Fallback: drop a leading conference year (the UI shows year separately).
  return v.replace(/^(?:19|20)\d{2}\s+/, '');
}
