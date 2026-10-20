import { PdfActionType, PdfZoomMode } from '@embedpdf/models';
import { describe, expect, it } from 'vitest';
import { loadCitations, type EngineLike } from './load';

function task<T>(v: T) { return { toPromise: () => Promise.resolve(v) }; }

// Minimal fake doc: 2 pages, 800pt tall. EmbedPDF returns annotation and text
// rects in top-left device space (given here directly: heading y=400, reference
// entry y=430), but GoTo destination `y` in PDF bottom-left space — so the
// marker's destination y is 370 (= 800 − 430), which loadCitations flips back to
// 430 to line it up with the reference entry.
const doc: any = {
  id: 'd', pageCount: 2,
  pages: [
    { index: 0, size: { width: 600, height: 800 }, rotation: 0 },
    { index: 1, size: { width: 600, height: 800 }, rotation: 0 },
  ],
};

const engine: EngineLike = {
  getPageAnnotations: (_d, page: any) => task(
    page.index === 0
      ? [
          // a LINK marker on page 0 whose GoTo destination (bottom-left y=370)
          // flips to the reference entry (page 1, top-left y=430).
          { type: 2 /* LINK */, pageIndex: 0, rect: { origin: { x: 90, y: 100 }, size: { width: 12, height: 12 } },
            target: {
              type: 'destination',
              destination: { pageIndex: 1, view: [], zoom: { mode: PdfZoomMode.XYZ, params: { x: 50, y: 370, zoom: 0 } } },
            } },
        ]
      : [
          // a URI link inside the reference entry on page 1 (y=432, in its band)
          { type: 2 /* LINK */, pageIndex: 1, rect: { origin: { x: 300, y: 432 }, size: { width: 80, height: 12 } },
            target: { type: 'action', action: { type: PdfActionType.URI, uri: 'https://doi.org/10.1/adam' } } },
        ],
  ),
  getPageTextRuns: (_d, page: any) => task(
    page.index === 1
      ? { runs: [
          { text: 'References', rect: { origin: { x: 50, y: 400 }, size: { width: 90, height: 16 } } },
          { text: '[1] Kingma, Ba. Adam. ICLR 2015.', rect: { origin: { x: 50, y: 430 }, size: { width: 320, height: 12 } } },
        ] }
      : { runs: [{ text: 'body [1]', rect: { origin: { x: 50, y: 100 }, size: { width: 80, height: 12 } } }] },
  ),
};

describe('loadCitations', () => {
  it('produces one reference and one marker from engine annotations + text', async () => {
    const { references, markers } = await loadCitations(engine, doc);
    expect(references).toHaveLength(1);
    expect(references[0].rawText).toContain('Adam');
    expect(references[0].externalUrl).toBe('https://doi.org/10.1/adam');
    expect(markers).toHaveLength(1);
    expect(markers[0]).toMatchObject({ pageIndex: 0, refIndex: 0 });
  });

  it('returns empty when there is no references heading', async () => {
    const noRefEngine: EngineLike = {
      getPageAnnotations: engine.getPageAnnotations,
      getPageTextRuns: (_d, _p) => task({ runs: [{ text: 'body', rect: { origin: { x: 0, y: 0 }, size: { width: 1, height: 1 } } }] }),
    };
    const { references, markers } = await loadCitations(noRefEngine, doc);
    expect(references).toHaveLength(0);
    expect(markers).toHaveLength(0);
  });

  it('reads the destination top from view[] for FitH destinations', async () => {
    const eng: EngineLike = {
      getPageAnnotations: (_d, page: any) => task(
        page.index === 0
          ? [{ type: 2, rect: { origin: { x: 90, y: 100 }, size: { width: 12, height: 12 } },
               target: { type: 'destination',
                 destination: { pageIndex: 1, view: [370], zoom: { mode: PdfZoomMode.FitHorizontal } } } }]
          : [],
      ),
      getPageTextRuns: (_d, page: any) => task(
        page.index === 1
          ? { runs: [
              { text: 'References', rect: { origin: { x: 50, y: 400 }, size: { width: 90, height: 16 } } },
              { text: '[1] A. Author. Adam. 2015.', rect: { origin: { x: 50, y: 430 }, size: { width: 300, height: 12 } } },
            ] }
          : { runs: [] },
      ),
    };
    const data = await loadCitations(eng, doc);
    // view[0]=370 in bottom-left space flips to 800-370=430 → the entry line.
    expect(data.references).toHaveLength(1);
    expect(data.references[0].rawText).toContain('Adam');
    expect(data.markers).toHaveLength(1);
  });

  it('anchors whole-page-fit destinations to the page top, not the flipped bottom', async () => {
    const eng: EngineLike = {
      getPageAnnotations: (_d, page: any) => task(
        page.index === 0
          ? [{ type: 2, rect: { origin: { x: 90, y: 100 }, size: { width: 12, height: 12 } },
               target: { type: 'destination',
                 destination: { pageIndex: 1, view: [], zoom: { mode: PdfZoomMode.FitPage } } } }]
          : [],
      ),
      getPageTextRuns: (_d, page: any) => task(
        page.index === 1
          ? { runs: [
              { text: 'References', rect: { origin: { x: 50, y: 0 }, size: { width: 90, height: 16 } } },
              { text: '[1] A. Author. Adam. 2015.', rect: { origin: { x: 50, y: 30 }, size: { width: 300, height: 12 } } },
            ] }
          : { runs: [] },
      ),
    };
    const data = await loadCitations(eng, doc);
    // destY must be 0 (top): at/after the heading at y=0 within DEST_EPSILON
    // it still counts as a citation link and picks up the entry text below.
    expect(data.markers).toHaveLength(1);
    // At the buggy flipped-bottom destY (800) the text band [794, ∞) contains
    // no runs, so rawText would be empty — this discriminates top vs bottom.
    expect(data.references).toHaveLength(1);
    expect(data.references[0].rawText).toContain('Adam');
  });

  it('finds a heading on the page before the first destination page', async () => {
    const eng: EngineLike = {
      getPageAnnotations: (_d, page: any) => task(
        page.index === 0
          ? [{ type: 2, rect: { origin: { x: 90, y: 100 }, size: { width: 12, height: 12 } },
               target: { type: 'destination',
                 destination: { pageIndex: 1, view: [], zoom: { mode: PdfZoomMode.XYZ, params: { x: 50, y: 770, zoom: 0 } } } } }]
          : [],
      ),
      getPageTextRuns: (_d, page: any) => task(
        page.index === 0
          ? { runs: [{ text: 'References', rect: { origin: { x: 50, y: 780 }, size: { width: 90, height: 16 } } }] }
          : { runs: [{ text: '[1] A. Author. Adam. 2015.', rect: { origin: { x: 50, y: 30 }, size: { width: 300, height: 12 } } }] },
      ),
    };
    const data = await loadCitations(eng, doc);
    expect(data.references).toHaveLength(1);
    expect(data.references[0].rawText).toContain('Adam');
  });
});

describe('text-layer fallback (no hyperlinks)', () => {
  const doc3: any = {
    id: 'd3', pageCount: 3,
    pages: [0, 1, 2].map((i) => ({ index: i, size: { width: 600, height: 800 }, rotation: 0 })),
  };
  const noAnnos: EngineLike['getPageAnnotations'] = () => task([]);

  it('segments a numbered bibliography and finds [n] markers', async () => {
    const eng: EngineLike = {
      getPageAnnotations: noAnnos,
      getPageTextRuns: (_d, page: any) => task({
        runs: page.index === 0
          ? [{ text: 'we build on [1] and [2] here', rect: { origin: { x: 50, y: 100 }, size: { width: 250, height: 12 } } }]
          : page.index === 2
            ? [
                { text: 'References', rect: { origin: { x: 50, y: 40 }, size: { width: 90, height: 16 } } },
                { text: '[1] Kingma. Adam. ICLR 2015.', rect: { origin: { x: 50, y: 80 }, size: { width: 280, height: 12 } } },
                { text: '[2] Devlin. BERT. NAACL 2019.', rect: { origin: { x: 50, y: 110 }, size: { width: 280, height: 12 } } },
              ]
            : [],
      }),
    };
    const data = await loadCitations(eng, doc3);
    expect(data.references).toHaveLength(2);
    expect(data.markers).toHaveLength(2);
    expect(data.markers.map((m) => m.refIndex).sort()).toEqual([0, 1]);
  });

  it('returns author-year candidates as pending (resolved after the LLM parse)', async () => {
    const eng: EngineLike = {
      getPageAnnotations: noAnnos,
      getPageTextRuns: (_d, page: any) => task({
        runs: page.index === 0
          ? [{ text: 'following (Kingma and Ba, 2015) we', rect: { origin: { x: 50, y: 100 }, size: { width: 260, height: 12 } } }]
          : page.index === 2
            ? [
                { text: 'References', rect: { origin: { x: 50, y: 40 }, size: { width: 90, height: 16 } } },
                { text: 'Kingma, D. and Ba, J. (2015). Adam. ICLR.', rect: { origin: { x: 50, y: 80 }, size: { width: 300, height: 12 } } },
                { text: 'Devlin, J. (2019). BERT. NAACL.', rect: { origin: { x: 50, y: 110 }, size: { width: 280, height: 12 } } },
              ]
            : [],
      }),
    };
    const data = await loadCitations(eng, doc3);
    expect(data.references).toHaveLength(2);
    expect(data.markers).toHaveLength(0);
    expect(data.pendingAuthorYear).toHaveLength(1);
  });

  it('does not re-read pages the hyperlink path already fetched', async () => {
    const calls: number[] = [];
    const eng: EngineLike = {
      // A GoTo link exists, so the hyperlink path reads the destination pages —
      // but no heading is found and it falls through to the text fallback,
      // which rescans those same pages.
      getPageAnnotations: (_d, page: any) => task(
        page.index === 0
          ? [{ type: 2, rect: { origin: { x: 90, y: 100 }, size: { width: 12, height: 12 } },
               target: { type: 'destination',
                 destination: { pageIndex: 1, view: [], zoom: { mode: PdfZoomMode.XYZ, params: { x: 50, y: 370, zoom: 0 } } } } }]
          : [],
      ),
      getPageTextRuns: (_d, page: any) => {
        calls.push(page.index);
        return task({ runs: [{ text: 'no heading here', rect: { origin: { x: 50, y: 100 }, size: { width: 120, height: 12 } } }] });
      },
    };
    const data = await loadCitations(eng, doc);
    expect(data.references).toHaveLength(0);
    // One worker round-trip per page: the fallback's rescan must hit the
    // shared cache, not fetch again.
    expect([...calls].sort()).toEqual([0, 1]);
  });

  it('stays empty when there is no heading anywhere', async () => {
    const eng: EngineLike = {
      getPageAnnotations: noAnnos,
      getPageTextRuns: () => task({ runs: [{ text: 'just prose', rect: { origin: { x: 50, y: 100 }, size: { width: 100, height: 12 } } }] }),
    };
    const data = await loadCitations(eng, doc3);
    expect(data.references).toHaveLength(0);
    expect(data.markers).toHaveLength(0);
  });
});
