import { describe, expect, it } from 'vitest';
import { buildTitleIndex, matchReferences, normalizeTitle } from './match';
import type { Reference } from './references';

function ref(index: number, rawText: string): Reference {
  return { index, destPageIndex: 1, destY: 100 + index, rawText };
}

describe('normalizeTitle', () => {
  it('lowercases, strips punctuation, collapses whitespace (mirrors matching.rs)', () => {
    expect(normalizeTitle('KGAT: Knowledge-Graph  Attention Network!')).toBe(
      'kgat knowledge graph attention network',
    );
  });
});

describe('matchReferences', () => {
  const papers = [
    { id: 'p-adam', title: 'Adam: A Method for Stochastic Optimization' },
    { id: 'p-resnet', title: 'Deep Residual Learning for Image Recognition' },
    { id: 'p-empty', title: null },
  ];

  it('matches when a library title appears verbatim inside the reference text', () => {
    const refs = [ref(0, '[12] D. Kingma, J. Ba. Adam: A Method for Stochastic Optimization. ICLR 2015.')];
    const m = matchReferences(refs, buildTitleIndex(papers));
    expect(m.get(0)?.id).toBe('p-adam');
  });

  it('does not match unrelated references', () => {
    const refs = [ref(0, '[3] Some Unrelated Paper About Frogs. Nature 2001.')];
    expect(matchReferences(refs, buildTitleIndex(papers)).has(0)).toBe(false);
  });

  it('guards against very short titles causing false positives', () => {
    const shortTitlePapers = [{ id: 'p-x', title: 'On It' }];
    const refs = [ref(0, '[1] A paper that mentions on it somewhere in prose. 2020.')];
    expect(matchReferences(refs, buildTitleIndex(shortTitlePapers)).has(0)).toBe(false);
  });

  it('ignores papers with null titles', () => {
    const refs = [ref(0, 'anything at all')];
    expect(matchReferences(refs, buildTitleIndex(papers)).has(0)).toBe(false);
  });

  it('matches on the structured title even when the raw text differs', () => {
    const refs = [{
      ...ref(0, '[7] Kingma+Ba, 3rd Intl Conf on Learning Repr, San Diego, 2015.'),
      structured: {
        authors: ['D. Kingma'], title: 'Adam: A Method for Stochastic Optimization',
        venue: 'ICLR', year: 2015, doi: null, arxiv_id: null, url: null,
      },
    }];
    expect(matchReferences(refs, buildTitleIndex(papers)).get(0)?.id).toBe('p-adam');
  });

  it('structured null still falls back to substring matching', () => {
    const refs = [{ ...ref(0, 'x Adam: A Method for Stochastic Optimization x'), structured: null }];
    expect(matchReferences(refs, buildTitleIndex(papers)).get(0)?.id).toBe('p-adam');
  });

  it('breaks duplicate-title ties the same way as the substring path (first wins)', () => {
    const dupPapers = [
      { id: 'p-first', title: 'Adam: A Method for Stochastic Optimization' },
      { id: 'p-second', title: 'Adam: A Method for Stochastic Optimization' },
    ];
    const refs = [{
      ...ref(0, 'unrelated raw text without the title'),
      structured: {
        authors: [], title: 'Adam: A Method for Stochastic Optimization',
        venue: null, year: null, doi: null, arxiv_id: null, url: null,
      },
    }];
    expect(matchReferences(refs, buildTitleIndex(dupPapers)).get(0)?.id).toBe('p-first');
  });
});
