import { describe, expect, it } from 'vitest';
import { columnMajorLines, segmentReferences, findNumberedMarkers, entryHeadInfo, findAuthorYearCandidates, resolveAuthorYearMarkers } from './text';
import type { PageText } from './references';
import type { CmLine } from './text';

function page(pageIndex: number, width: number, height: number, runs: PageText['runs'], urlLinks: PageText['urlLinks'] = []): PageText {
  return { pageIndex, width, height, runs, urlLinks };
}

describe('columnMajorLines', () => {
  it('joins same-baseline runs and orders lines column-major', () => {
    const p = page(0, 600, 800, [
      { text: 'right col line', x: 320, y: 50, width: 100, height: 12 },
      { text: 'left ', x: 50, y: 50, width: 40, height: 12 },
      { text: 'col line', x: 90, y: 51, width: 60, height: 12 },
    ]);
    const lines = columnMajorLines(p);
    expect(lines.map((l) => l.text)).toEqual(['left col line', 'right col line']);
    expect(lines[0].runs).toHaveLength(2);
    expect(lines[0].runs[1].start).toBe(5); // char offset of 'col line'
  });

  it('reassembles a small-caps heading split across y-buckets (jimenez drop-cap geometry)', () => {
    // Same Mode-B geometry as citations/references.test.ts: "R" (y=81, h=14) and
    // "EFERENCES" (y=83, h=12) share a baseline (bottom 95) but land in
    // different Math.round(y/3) buckets. columnMajorLines must merge them so the
    // fallback path's segmentation sees one "REFERENCES" line, not two fragments.
    const p = page(10, 612, 792, [
      { text: 'R', x: 108, y: 81, width: 12, height: 14 },
      { text: 'EFERENCES', x: 117, y: 83, width: 82, height: 12 },
    ]);
    const lines = columnMajorLines(p);
    expect(lines).toHaveLength(1);
    expect(lines[0].text).toBe('REFERENCES');
    expect(lines[0].runs).toHaveLength(2);
    expect(lines[0].runs[1].start).toBe(1); // 'EFERENCES' begins after 'R'
  });

  it('does not merge a drop-cap heading with same-band text in the OTHER column (kim)', () => {
    // Two-column IEEE (mid=306): the split "R"+"EFERENCES" heading sits in the
    // left column while the right column carries body text on the same baseline.
    // Column-aware clustering must keep them as two distinct lines.
    const p = page(15, 612, 792, [
      { text: 'R', x: 55, y: 430, width: 12, height: 14 },
      { text: 'EFERENCES', x: 64, y: 432, width: 82, height: 12 },
      { text: 'runtime monitoring with s-taliro', x: 320, y: 431, width: 230, height: 12 },
    ]);
    const lines = columnMajorLines(p);
    expect(lines.map((l) => l.text)).toEqual(['REFERENCES', 'runtime monitoring with s-taliro']);
    expect(lines[0].col).toBe(0);
    expect(lines[1].col).toBe(1);
  });
});

describe('segmentReferences — numbered', () => {
  const refStart = { pageIndex: 2, y: 40, x: 50 };
  const bib = page(2, 600, 800, [
    { text: 'References', x: 50, y: 40, width: 90, height: 16 },
    { text: '[1] D. Kingma, J. Ba. Adam: A Method for', x: 50, y: 80, width: 300, height: 12 },
    { text: 'Stochastic Optimization. ICLR 2015.', x: 62, y: 100, width: 280, height: 12 },
    { text: '[2] J. Devlin et al. BERT. NAACL 2019.', x: 50, y: 130, width: 300, height: 12 },
  ], [{ x: 200, y: 100, width: 60, height: 12, url: 'https://arxiv.org/abs/1412.6980' }]);

  it('splits entries at [n] line starts and joins continuation lines', () => {
    const seg = segmentReferences([bib], refStart)!;
    expect(seg.style).toBe('numbered');
    expect(seg.references).toHaveLength(2);
    expect(seg.references[0].rawText).toBe('[1] D. Kingma, J. Ba. Adam: A Method for Stochastic Optimization. ICLR 2015.');
    expect(seg.references[0].externalUrl).toBe('https://arxiv.org/abs/1412.6980');
    expect(seg.numberOf.get(1)).toBe(0);
    expect(seg.numberOf.get(2)).toBe(1);
  });

  it('does not attach a same-baseline URL from the other column', () => {
    // Two-column page (mid=300): the left column's entry shares its baseline
    // with a DOI link that belongs to the RIGHT column's entry.
    const twoCol = page(2, 600, 800, [
      { text: 'References', x: 50, y: 40, width: 90, height: 16 },
      { text: '[1] Left entry. 2020.', x: 50, y: 80, width: 180, height: 12 },
      { text: '[2] Right entry. 2021.', x: 320, y: 80, width: 200, height: 12 },
    ], [{ x: 340, y: 80, width: 60, height: 12, url: 'https://doi.org/10.1/right' }]);
    const seg = segmentReferences([twoCol], refStart)!;
    expect(seg.references).toHaveLength(2);
    expect(seg.references[0].externalUrl).toBeUndefined();
    expect(seg.references[1].externalUrl).toBe('https://doi.org/10.1/right');
  });

  it('returns null for a single [n] line (below the MIN_ENTRIES=2 guard)', () => {
    const tiny = page(0, 600, 800, [
      { text: 'References', x: 50, y: 40, width: 90, height: 16 },
      { text: '[1] Only one.', x: 50, y: 80, width: 120, height: 12 },
    ]);
    expect(segmentReferences([tiny], { pageIndex: 0, y: 40, x: 50 })).toBeNull();
  });

  it('includes right-column entries above the heading y (column-major, two-column page)', () => {
    // 600pt page, mid=300: heading + [1] in the LEFT column, [2] in the RIGHT
    // column at y=30 — above the heading's y=60, but AFTER it in reading order.
    const bib = page(2, 600, 800, [
      { text: 'References', x: 50, y: 60, width: 90, height: 16 },
      { text: '[1] Left entry. 2020.', x: 50, y: 100, width: 180, height: 12 },
      { text: '[2] Right entry above heading y. 2021.', x: 320, y: 30, width: 240, height: 12 },
    ]);
    const seg = segmentReferences([bib], { pageIndex: 2, y: 60, x: 50 })!;
    expect(seg).not.toBeNull();
    expect(seg.style).toBe('numbered');
    expect(seg.references).toHaveLength(2);
    expect(seg.numberOf.get(1)).toBe(0);
    expect(seg.numberOf.get(2)).toBe(1);
    expect(seg.references[1].rawText).toBe('[2] Right entry above heading y. 2021.');
  });
});

describe('segmentReferences — author-year (hanging indent)', () => {
  const refStart = { pageIndex: 2, y: 40, x: 50 };

  it('splits entries at flush-left lines (continuations indented)', () => {
    const bib = page(2, 600, 800, [
      { text: 'References', x: 50, y: 40, width: 90, height: 16 },
      { text: 'Kingma, D. and Ba, J. (2015). Adam: a method', x: 50, y: 80, width: 300, height: 12 },
      { text: 'for stochastic optimization. In ICLR.', x: 68, y: 100, width: 260, height: 12 },
      { text: 'Devlin, J. et al. (2019). BERT. In NAACL.', x: 50, y: 130, width: 300, height: 12 },
      { text: 'Vaswani, A. (2017). Attention is all you need.', x: 50, y: 160, width: 300, height: 12 },
    ]);
    const seg = segmentReferences([bib], refStart)!;
    expect(seg.style).toBe('authoryear');
    expect(seg.references).toHaveLength(3);
    expect(seg.references[0].rawText).toBe(
      'Kingma, D. and Ba, J. (2015). Adam: a method for stochastic optimization. In ICLR.',
    );
  });

  it('handles the inverted pattern (first line indented, continuations flush)', () => {
    const bib = page(2, 600, 800, [
      { text: 'References', x: 50, y: 40, width: 90, height: 16 },
      { text: 'Kingma, D. (2015). Adam: a method for', x: 68, y: 80, width: 280, height: 12 },
      { text: 'stochastic optimization. ICLR.', x: 50, y: 100, width: 240, height: 12 },
      { text: 'Devlin, J. (2019). BERT. NAACL.', x: 68, y: 130, width: 260, height: 12 },
    ]);
    const seg = segmentReferences([bib], refStart)!;
    expect(seg.style).toBe('authoryear');
    expect(seg.references).toHaveLength(2);
  });

  it('rejects blocks where most entries lack a year (not a bibliography)', () => {
    const notBib = page(2, 600, 800, [
      { text: 'References', x: 50, y: 40, width: 90, height: 16 },
      { text: 'Some sentence without anything.', x: 50, y: 80, width: 280, height: 12 },
      { text: 'Another plain sentence here too.', x: 50, y: 110, width: 280, height: 12 },
    ]);
    expect(segmentReferences([notBib], refStart)).toBeNull();
  });

  it('segments two-column author-year bibliographies per column', () => {
    // Left column: starts x=50, one continuation line at x=68. Right column:
    // starts x=320, one continuation line at x=338. Globally the x=50 bucket
    // (heading + 2 entry starts) outranks x=320 (2 entry starts), so the old
    // global top-2-buckets logic picks {50, 320} as candidates and then the
    // year-share tiebreak (right column is 100% years vs left's 67%, since
    // "References" has no year) selects x=320 as the SOLE start column —
    // dropping both left-column entries entirely and returning only 2
    // references instead of 4. Per-column detection picks each column's own
    // start-x independently and recovers all 4 entries.
    const bib = page(2, 600, 800, [
      { text: 'References', x: 50, y: 40, width: 90, height: 16 },
      { text: 'Kingma, D. (2015). Adam: a method', x: 50, y: 80, width: 220, height: 12 },
      { text: 'for stochastic optimization. ICLR.', x: 68, y: 100, width: 200, height: 12 },
      { text: 'Devlin, J. (2019). BERT. NAACL.', x: 50, y: 130, width: 220, height: 12 },
      { text: 'He, K. (2016). ResNet. CVPR.', x: 320, y: 40, width: 200, height: 12 },
      { text: 'Vaswani, A. (2017). Attention is all', x: 320, y: 70, width: 220, height: 12 },
      { text: 'you need. NeurIPS.', x: 338, y: 90, width: 140, height: 12 },
    ]);
    const seg = segmentReferences([bib], { pageIndex: 2, y: 40, x: 50 })!;
    expect(seg).not.toBeNull();
    expect(seg.style).toBe('authoryear');
    expect(seg.references).toHaveLength(4);
    expect(seg.references[2].rawText).toBe('He, K. (2016). ResNet. CVPR.');
    expect(seg.references[3].rawText).toBe('Vaswani, A. (2017). Attention is all you need. NeurIPS.');
  });
});

describe('findNumberedMarkers', () => {
  const numberOf = new Map([[1, 0], [2, 1], [3, 2], [4, 3], [5, 4]]);
  const line = (text: string, x = 50, y = 100): CmLine => {
    const run = { text, x, y, width: text.length * 5, height: 12 };
    return { pageIndex: 0, col: 0, y, x, text, runs: [{ run, start: 0, end: text.length }] };
  };

  it('finds [3] and maps it to the entry', () => {
    const ms = findNumberedMarkers([line('as shown in [3] recently')], numberOf);
    expect(ms).toHaveLength(1);
    expect(ms[0].refIndex).toBe(2);
  });

  it('gives each list member its own marker; a range keeps one at its first entry', () => {
    // Live bug (empc, S&P'25): [9,15,22,40,46,47]-style groups produced ONE
    // marker at the first ref, so the other members were never hoverable
    // anywhere in the paper.
    const ms = findNumberedMarkers([line('prior work [3, 5] and [1–4]')], numberOf);
    expect(ms).toHaveLength(3);
    expect(ms.map((m) => m.refIndex)).toEqual([2, 4, 0]);
  });

  it('gives each list member a rect over its own number', () => {
    const l = line('cf. [3, 5] here'); // '3' at char 5, '5' at char 8; 5px/char
    const ms = findNumberedMarkers([l], numberOf);
    expect(ms.map((m) => m.refIndex)).toEqual([2, 4]);
    expect(ms[0].x).toBeCloseTo(50 + 5 * 5, 1);
    expect(ms[0].width).toBeCloseTo(5, 1);
    expect(ms[1].x).toBeCloseTo(50 + 8 * 5, 1);
    expect(ms[1].width).toBeCloseTo(5, 1);
  });

  it('joins a bracket group split across a line break (same page and column)', () => {
    // Live bug (empc): "…techniques [14,19,37,\n44]…" matched nothing — none
    // of the four refs got a marker from this cite.
    const a = line('techniques [3,', 50, 100);
    const b = line('5] and more', 50, 115);
    const ms = findNumberedMarkers([a, b], numberOf);
    expect(ms.map((m) => m.refIndex)).toEqual([2, 4]);
    expect(ms[0].y).toBe(100); // the '3,' part sits on line a
    expect(ms[1].y).toBe(115); // the '5]' part sits on line b
  });

  it('validates a joined group as a whole and respects line adjacency', () => {
    // [0,\n1] is still math, not a citation.
    const a = line('interval [0,', 50, 100);
    const b = line('1] normalization', 50, 115);
    expect(findNumberedMarkers([a, b], numberOf)).toHaveLength(0);
    // A column break between the fragments means no join.
    const c = line('techniques [3,', 50, 100);
    const d = { ...line('5] and more', 50, 115), col: 1 };
    expect(findNumberedMarkers([c, d], numberOf)).toHaveLength(0);
  });

  it('rejects math intervals like [0, 1] and out-of-range numbers', () => {
    expect(findNumberedMarkers([line('in the interval [0, 1] we')], numberOf)).toHaveLength(0);
    expect(findNumberedMarkers([line('see [17]')], numberOf)).toHaveLength(0);
  });

  it('computes a proportional rect inside the line', () => {
    const l = line('abcd [3] xyz'); // '[3]' at chars 5..8 of a 12-char run, width 60
    const [m] = findNumberedMarkers([l], numberOf);
    expect(m.x).toBeCloseTo(50 + (5 / 12) * 60, 1);
    expect(m.width).toBeCloseTo((3 / 12) * 60, 1);
    expect(m.y).toBe(100);
    expect(m.height).toBe(12);
  });
});

describe('author-year markers', () => {
  const line = (text: string, x = 50, y = 100): CmLine => {
    const run = { text, x, y, width: text.length * 5, height: 12 };
    return { pageIndex: 0, col: 0, y, x, text, runs: [{ run, start: 0, end: text.length }] };
  };
  const refs = [
    { index: 0, destPageIndex: 2, destY: 80, rawText: 'Kingma, D. and Ba, J. (2015). Adam.' },
    { index: 1, destPageIndex: 2, destY: 130, rawText: 'Devlin, J. et al. (2019). BERT.',
      structured: { authors: ['Jacob Devlin'], title: 'BERT', venue: null, year: 2019, doi: null, arxiv_id: null, url: null } },
  ];

  it('entryHeadInfo pulls the first surname and year from a raw entry', () => {
    expect(entryHeadInfo('Kingma, D. and Ba, J. (2015). Adam.')).toEqual({ surname: 'kingma', year: 2015 });
  });

  it('finds parenthetical candidates and resolves multi-cites to the first hit', () => {
    const cands = findAuthorYearCandidates([line('as shown (Kingma and Ba, 2015; Devlin et al., 2019) here')]);
    expect(cands).toHaveLength(1);
    const ms = resolveAuthorYearMarkers(cands, refs as never);
    expect(ms).toHaveLength(1);
    expect(ms[0].refIndex).toBe(0);
  });

  it('resolves via structured authors when available (narrative cite)', () => {
    const cands = findAuthorYearCandidates([line('Devlin et al. (2019) show that')]);
    expect(cands).toHaveLength(1);
    const ms = resolveAuthorYearMarkers(cands, refs as never);
    expect(ms[0].refIndex).toBe(1);
  });

  it('ignores year-only parentheses that match no entry', () => {
    const cands = findAuthorYearCandidates([line('since (2015) alone means nothing')]);
    expect(resolveAuthorYearMarkers(cands, refs as never)).toHaveLength(0);
  });

  it('handles typographic (curly) apostrophes in surnames', () => {
    const curlyRefs = [
      { index: 0, destPageIndex: 2, destY: 80, rawText: 'O’Brien, P. (2020). A Paper.' },
    ];
    const cands = findAuthorYearCandidates([line('as shown (O’Brien, 2020) here')]);
    expect(cands).toHaveLength(1);
    const ms = resolveAuthorYearMarkers(cands, curlyRefs as never);
    expect(ms).toHaveLength(1);
    expect(ms[0].refIndex).toBe(0);
  });

  it('entryHeadInfo keeps curly-apostrophe surnames whole', () => {
    expect(entryHeadInfo('O’Brien, P. (2020). A Paper.')).toEqual({ surname: 'o’brien', year: 2020 });
  });
});
