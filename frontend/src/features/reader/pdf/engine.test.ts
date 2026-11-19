import { describe, expect, it } from 'vitest';
import { DOCUMENT_SCOPE, ENGINE_OPTIONS, IDENTITY, viewerPlugins } from './engine';

describe('ENGINE_OPTIONS', () => {
  it('fetches no fallback fonts, so the reader works offline', () => {
    expect(ENGINE_OPTIONS.defaultFallbackFonts).toBe(false);
  });
});

describe('DOCUMENT_SCOPE', () => {
  it('keeps the file’s own restrictions and adds only what the reader needs', () => {
    expect([...DOCUMENT_SCOPE].sort()).toEqual(
      ['doc.annotate.import', 'doc.download', 'doc.text.copy', 'pdf.permissions'].sort(),
    );
  });
});

describe('IDENTITY', () => {
  it('names an author that is not the OS user', () => {
    // An exported copy carries this; it must never leak a real name.
    expect(IDENTITY.displayName).toBe('Xuewen');
  });
});

describe('viewerPlugins', () => {
  const byId = new Map(viewerPlugins().map((p) => [p.id, p]));

  it('includes every plugin the reader uses', () => {
    for (const id of [
      'stage', 'stage-thumbs', 'render', 'interaction', 'selection', 'search',
      'outline', 'link', 'history', 'annotation', 'page-text',
    ]) {
      expect([...byId.keys()]).toContain(id);
    }
  });

  it('keeps the tool armed after a mark so passages can be marked in a row', () => {
    expect(byId.get('annotation')?.settings?.registered).toMatchObject({ afterCreate: { select: true, tool: 'stay' } });
  });
});
