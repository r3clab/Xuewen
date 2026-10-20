import { describe, expect, it } from 'vitest';
import { PdfAnnotationSubtype } from '@embedpdf/models';
import type { PdfAnnotationObject } from '@embedpdf/models';
import { ANNOTATION_RENDERERS, SUPPRESS_LINK_RENDERER } from './renderers';

function anno(type: PdfAnnotationSubtype): PdfAnnotationObject {
  return {
    id: 'a1',
    type,
    pageIndex: 0,
    rect: { origin: { x: 0, y: 0 }, size: { width: 1, height: 1 } },
  } as PdfAnnotationObject;
}

describe('SUPPRESS_LINK_RENDERER', () => {
  it("claims the built-in link renderer's id", () => {
    // The layer drops a built-in whose id an external entry claims, so the id
    // must stay exactly 'link' or the built-in survives alongside ours.
    expect(SUPPRESS_LINK_RENDERER.id).toBe('link');
  });

  it('matches nothing, so links end up with no renderer at all', () => {
    for (const type of [
      PdfAnnotationSubtype.LINK,
      PdfAnnotationSubtype.HIGHLIGHT,
      PdfAnnotationSubtype.TEXT,
    ]) {
      expect(SUPPRESS_LINK_RENDERER.matches?.(anno(type))).toBe(false);
    }
  });

  it('keeps the null-rendering stub, not a real component', () => {
    // createRenderer fills `component` in, so it is never undefined; what
    // matters is that it is still the stub that draws nothing.
    expect((SUPPRESS_LINK_RENDERER.component as () => unknown)()).toBeNull();
  });
});

describe('ANNOTATION_RENDERERS', () => {
  it('carries the link suppression and nothing that shadows a mark we draw', () => {
    expect(ANNOTATION_RENDERERS).toContain(SUPPRESS_LINK_RENDERER);
    // Claiming e.g. 'highlight' here would silently replace the renderer that
    // actually paints our marks.
    const ids = ANNOTATION_RENDERERS.map((r) => r.id);
    expect(ids).toEqual(['link']);
  });
});
