import { describe, expect, it } from 'vitest';
import { abbreviateVenue } from './venue';

describe('abbreviateVenue', () => {
  it('maps a full name to its canonical abbreviation', () => {
    expect(abbreviateVenue('2025 IEEE Symposium on Security and Privacy (SP)')).toBe('S&P');
  });

  it('canonicalizes a bare acronym found in the string (S&P)', () => {
    expect(abbreviateVenue('IEEE S&P')).toBe('S&P');
  });

  it('maps Neural Information Processing Systems to NeurIPS', () => {
    expect(abbreviateVenue('Advances in Neural Information Processing Systems')).toBe('NeurIPS');
  });

  it('recognizes an already-short acronym via the curated map (ICML)', () => {
    expect(abbreviateVenue('ICML')).toBe('ICML');
  });

  it('canonicalizes a messy real acronym (NAACL-HLT → NAACL)', () => {
    expect(abbreviateVenue('NAACL-HLT')).toBe('NAACL');
  });

  it('maps the full ISSTA proceedings name to ISSTA', () => {
    expect(
      abbreviateVenue(
        'Proceedings of the 31st ACM SIGSOFT International Symposium on Software Testing and Analysis',
      ),
    ).toBe('ISSTA');
  });

  it('maps every POPL spelling to POPL', () => {
    expect(abbreviateVenue('POPL 2026')).toBe('POPL');
    expect(
      abbreviateVenue('Proceedings of the 53rd ACM SIGPLAN Symposium on Principles of Programming Languages'),
    ).toBe('POPL');
    expect(abbreviateVenue('Proc. ACM Program. Lang. 8(POPL)')).toBe('POPL');
    // What the resolver now stores: the journal plus the issue that names
    // the conference (src/metadata/resolve/mod.rs: venue_with_issue).
    expect(abbreviateVenue('Proceedings of the ACM on Programming Languages (POPL)')).toBe('POPL');
  });

  it('falls back to PACMPL only when no issue names a conference', () => {
    expect(abbreviateVenue('Proceedings of the ACM on Programming Languages')).toBe('PACMPL');
    expect(abbreviateVenue('Proc. ACM Program. Lang. 9(ICFP)')).toBe('ICFP');
  });

  it('maps USENIX Security to SEC', () => {
    expect(abbreviateVenue('USENIX Security Symposium')).toBe('SEC');
  });

  it('maps the Artificial Intelligence journal to AIJ', () => {
    expect(abbreviateVenue('Artificial Intelligence')).toBe('AIJ');
    expect(abbreviateVenue('Artif. Intell.')).toBe('AIJ');
  });

  it('leaves other journals with Artificial Intelligence in the name alone', () => {
    expect(abbreviateVenue('Journal of Artificial Intelligence Research')).toBe(
      'Journal of Artificial Intelligence Research',
    );
    expect(abbreviateVenue('Artificial Intelligence Review')).toBe('Artificial Intelligence Review');
  });

  it('falls back to a trailing parenthetical acronym when unmapped', () => {
    expect(abbreviateVenue('2024 Conference on Made Up Things (CMUT)')).toBe('CMUT');
  });

  it('strips a leading year when unmapped and has no parenthetical', () => {
    expect(abbreviateVenue('2019 Journal of Obscure Studies')).toBe('Journal of Obscure Studies');
  });

  it('passes an unlisted bare acronym through unchanged', () => {
    expect(abbreviateVenue('FLARB')).toBe('FLARB');
  });

  it('returns null and empty unchanged', () => {
    expect(abbreviateVenue(null)).toBeNull();
    expect(abbreviateVenue('')).toBe('');
  });
});
