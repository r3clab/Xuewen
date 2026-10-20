import { describe, expect, it } from 'vitest';
import { ANNOTATION_OPTIONS, ENGINE_OPTIONS, viewerPlugins } from './engine';
import { TOOL_BY_KIND } from '$features/reader/annotations/adapter';
import { ANNOTATION_COLORS, colorHex } from '$features/reader/annotations/palette';

describe('ENGINE_OPTIONS', () => {
  it('is offline + runs PDFium in a worker (load-bearing)', () => {
    expect(ENGINE_OPTIONS.worker).toBe(true);
    // Resolved to a fully-qualified URL (not a bare path) — the stock blob
    // worker's self.location is a blob: URL, which can't resolve a
    // path-absolute fetch like '/pdfium.wasm' against it. See pdf/engine.ts.
    expect(ENGINE_OPTIONS.wasmUrl.startsWith('http')).toBe(true);
    // Self-hosted: same origin, never the jsDelivr CDN default (which breaks
    // offline). The exact path is Vite's to choose — it fingerprints the file
    // so the backend can serve it immutable — so assert the shape, not a
    // literal name.
    expect(new URL(ENGINE_OPTIONS.wasmUrl).origin).toBe(location.origin);
    expect(ENGINE_OPTIONS.wasmUrl.endsWith('.wasm')).toBe(true);
    expect(ENGINE_OPTIONS.fontFallback).toBeNull();
  });
});

describe('viewerPlugins', () => {
  it('includes the needed plugins and opens no document up front', () => {
    const regs = viewerPlugins();
    // Every registration exposes a package manifest with an id.
    const ids = regs.map((r) => r.package.manifest.id);
    for (const id of [
      'viewport', 'scroll', 'render', 'selection', 'interaction-manager',
      'document-manager', 'tiling', 'search', 'thumbnail', 'bookmark',
      'annotation', 'history',
    ]) {
      expect(ids).toContain(id);
    }
    const docReg = regs.find((r) => r.package.manifest.id === 'document-manager');
    // Documents are opened per tab at runtime, not seeded here.
    expect(docReg?.config?.initialDocuments).toBeUndefined();
    // A cap high enough for many open tabs.
    expect(docReg?.config?.maxDocuments).toBeGreaterThanOrEqual(16);
    const thumbReg = regs.find((r) => r.package.manifest.id === 'thumbnail');
    // Continuous auto-follow snaps the pane to the current page on every
    // page change, fighting manual thumbnail browsing (trackpad momentum
    // keeps firing page changes). The side panel positions the pane once
    // when it opens instead.
    expect(thumbReg?.config?.autoScroll).toBe(false);
  });

  it('registers annotations with autoCommit off — the PDF is never written', () => {
    const reg = viewerPlugins().find((r) => r.package.manifest.id === 'annotation');
    // This is the setting the whole feature rests on: autoCommit would push
    // marks back through PDFium and change the bytes under content_hash.
    expect(reg?.config?.autoCommit).toBe(false);
    expect(ANNOTATION_OPTIONS.autoCommit).toBe(false);
  });

  it("leaves link navigation to the reader's own citation popovers", () => {
    expect(ANNOTATION_OPTIONS.autoOpenLinks).toBe(false);
  });

  it('keeps the tool armed after a mark so passages can be marked in a row', () => {
    expect(ANNOTATION_OPTIONS.deactivateToolAfterCreate).toBe(false);
    expect(ANNOTATION_OPTIONS.selectAfterCreate).toBe(true);
  });

  it('seeds every surfaced tool with a palette color', () => {
    const byId = new Map(ANNOTATION_OPTIONS.tools.map((t) => [t.id, t.defaults]));
    expect([...byId.keys()].sort()).toEqual(Object.values(TOOL_BY_KIND).sort());
    const palette = ANNOTATION_COLORS.map(colorHex);
    for (const [id, defaults] of byId) {
      const used = Object.values(defaults);
      expect(used.length, `${id} has no color`).toBeGreaterThan(0);
      for (const hex of used) expect(palette, `${id} uses ${hex}`).toContain(hex);
    }
    // A sticky note is an icon: stroke only, no fill to color.
    expect(byId.get(TOOL_BY_KIND.text_comment)).not.toHaveProperty('color');
  });

  it('names an author that is not the OS user', () => {
    // An exported copy carries this; it must never leak a real name.
    expect(ANNOTATION_OPTIONS.annotationAuthor).toBe('Xuewen');
  });
});
