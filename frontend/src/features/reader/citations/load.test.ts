import { describe, expect, it } from 'vitest';
import type { PdfLinkTarget } from '@embedpdf/svelte/link';
import type { PageRef } from '@embedpdf/svelte/runtime';
import { loadCitations, type CitationLink, type CitationSource } from './load';
import type { TextRun } from './references';

const page = (index: number): PageRef => ({ kind: 'objectNumber', objectNumber: 100 + index });

function run(text: string, x: number, y: number, width: number, height: number): TextRun {
  return { text, x, y, width, height };
}

function xyz(pageIndex: number, x: number, y: number): PdfLinkTarget {
  return { kind: 'goto', destination: { kind: 'xyz', page: page(pageIndex), x, y, zoom: null } };
}

function link(pageIndex: number, x: number, y: number, width: number, height: number, target: PdfLinkTarget | null): CitationLink {
  return { pageIndex, rect: { x, y, width, height }, target };
}

/// Pages 600×800pt. Every position — link rects, destinations, text runs —
/// is top-left page space, as the engine reads them.
function source(
  pageCount: number,
  links: readonly CitationLink[],
  runs: (pageIndex: number) => TextRun[],
  reads: number[] = [],
): CitationSource {
  return {
    pages: Array.from({ length: pageCount }, () => ({ width: 600, height: 800 })),
    links: () => links,
    readRuns: async (pageIndex) => {
      reads.push(pageIndex);
      return runs(pageIndex);
    },
    pageIndexOf: (ref) => (ref.objectNumber >= 100 && ref.objectNumber < 100 + pageCount ? ref.objectNumber - 100 : -1),
  };
}

// The heading at y=400 and the reference entry at y=430 on page 1; the
// marker's destination points at the entry's line.
const bibliography = (pageIndex: number) =>
  pageIndex === 1
    ? [run('References', 50, 400, 90, 16), run('[1] Kingma, Ba. Adam. ICLR 2015.', 50, 430, 320, 12)]
    : [run('body [1]', 50, 100, 80, 12)];

describe('loadCitations', () => {
  it('produces one reference and one marker from link annotations + text', async () => {
    const { references, markers } = await loadCitations(
      source(
        2,
        [
          link(0, 90, 100, 12, 12, xyz(1, 50, 430)),
          // A URI link inside the reference entry (y=432, in its band).
          link(1, 300, 432, 80, 12, { kind: 'uri', uri: 'https://doi.org/10.1/adam' }),
        ],
        bibliography,
      ),
    );
    expect(references).toHaveLength(1);
    expect(references[0].rawText).toContain('Adam');
    expect(references[0].externalUrl).toBe('https://doi.org/10.1/adam');
    expect(markers).toHaveLength(1);
    expect(markers[0]).toMatchObject({ pageIndex: 0, refIndex: 0 });
  });

  it('returns empty when there is no references heading', async () => {
    const { references, markers } = await loadCitations(
      source(2, [link(0, 90, 100, 12, 12, xyz(1, 50, 430))], () => [run('body', 0, 0, 1, 1)]),
    );
    expect(references).toHaveLength(0);
    expect(markers).toHaveLength(0);
  });

  it('reads the destination top of a FitH destination', async () => {
    const data = await loadCitations(
      source(2, [link(0, 90, 100, 12, 12, { kind: 'goto', destination: { kind: 'fitH', page: page(1), y: 430 } })], (i) =>
        i === 1 ? [run('References', 50, 400, 90, 16), run('[1] A. Author. Adam. 2015.', 50, 430, 300, 12)] : [],
      ),
    );
    expect(data.references).toHaveLength(1);
    expect(data.references[0].rawText).toContain('Adam');
    expect(data.markers).toHaveLength(1);
  });

  it('anchors whole-page-fit destinations to the page top', async () => {
    const data = await loadCitations(
      source(2, [link(0, 90, 100, 12, 12, { kind: 'goto', destination: { kind: 'fit', page: page(1) } })], (i) =>
        i === 1 ? [run('References', 50, 0, 90, 16), run('[1] A. Author. Adam. 2015.', 50, 30, 300, 12)] : [],
      ),
    );
    // destY must be 0 (top): at/after the heading at y=0 it still counts as a
    // citation link and picks up the entry text below.
    expect(data.markers).toHaveLength(1);
    expect(data.references).toHaveLength(1);
    expect(data.references[0].rawText).toContain('Adam');
  });

  it('ignores a link to a page the document does not have, or to another file', async () => {
    const data = await loadCitations(
      source(
        2,
        [
          link(0, 90, 100, 12, 12, xyz(7, 50, 430)),
          link(0, 90, 120, 12, 12, { kind: 'named', name: 'NextPage' }),
          link(0, 90, 140, 12, 12, null),
        ],
        bibliography,
      ),
    );
    expect(data.markers).toHaveLength(0);
  });

  it('finds a heading on the page before the first destination page', async () => {
    const data = await loadCitations(
      source(2, [link(0, 90, 100, 12, 12, xyz(1, 50, 30))], (i) =>
        i === 0 ? [run('References', 50, 780, 90, 16)] : [run('[1] A. Author. Adam. 2015.', 50, 30, 300, 12)],
      ),
    );
    expect(data.references).toHaveLength(1);
    expect(data.references[0].rawText).toContain('Adam');
  });
});

describe('text-layer fallback (no hyperlinks)', () => {
  it('segments a numbered bibliography and finds [n] markers', async () => {
    const data = await loadCitations(
      source(3, [], (i) =>
        i === 0
          ? [run('we build on [1] and [2] here', 50, 100, 250, 12)]
          : i === 2
            ? [
                run('References', 50, 40, 90, 16),
                run('[1] Kingma. Adam. ICLR 2015.', 50, 80, 280, 12),
                run('[2] Devlin. BERT. NAACL 2019.', 50, 110, 280, 12),
              ]
            : [],
      ),
    );
    expect(data.references).toHaveLength(2);
    expect(data.markers).toHaveLength(2);
    expect(data.markers.map((m) => m.refIndex).sort()).toEqual([0, 1]);
  });

  it('returns author-year candidates as pending (resolved after the LLM parse)', async () => {
    const data = await loadCitations(
      source(3, [], (i) =>
        i === 0
          ? [run('following (Kingma and Ba, 2015) we', 50, 100, 260, 12)]
          : i === 2
            ? [
                run('References', 50, 40, 90, 16),
                run('Kingma, D. and Ba, J. (2015). Adam. ICLR.', 50, 80, 300, 12),
                run('Devlin, J. (2019). BERT. NAACL.', 50, 110, 280, 12),
              ]
            : [],
      ),
    );
    expect(data.references).toHaveLength(2);
    expect(data.markers).toHaveLength(0);
    expect(data.pendingAuthorYear).toHaveLength(1);
  });

  it('does not re-read pages the hyperlink path already fetched', async () => {
    const reads: number[] = [];
    // A GoTo link exists, so the hyperlink path reads the destination pages —
    // but no heading is found and it falls through to the text fallback,
    // which rescans those same pages.
    const data = await loadCitations(
      source(2, [link(0, 90, 100, 12, 12, xyz(1, 50, 430))], () => [run('no heading here', 50, 100, 120, 12)], reads),
    );
    expect(data.references).toHaveLength(0);
    // One worker round-trip per page: the fallback's rescan must hit the
    // shared cache, not fetch again.
    expect([...reads].sort()).toEqual([0, 1]);
  });

  it('stays empty when there is no heading anywhere', async () => {
    const data = await loadCitations(source(3, [], () => [run('just prose', 50, 100, 100, 12)]));
    expect(data.references).toHaveLength(0);
    expect(data.markers).toHaveLength(0);
  });
});
