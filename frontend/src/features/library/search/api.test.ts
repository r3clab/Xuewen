import { describe, expect, it } from 'vitest';
import { searchParams } from './api';
import type { SearchOpts } from './types';

const allOpts: SearchOpts = {
  title: true,
  authors: true,
  abstract: true,
  body: true,
  notes: true,
  keyword: true,
  semantic: true,
};

describe('searchParams', () => {
  it('sends only q when both engines are selected', () => {
    const p = searchParams('fuzzing', allOpts);
    expect(p.get('q')).toBe('fuzzing');
    expect(p.get('engines')).toBeNull();
    expect([...p.keys()]).toEqual(['q']);
  });

  it('carries the raw query string verbatim (qualifiers parse server-side)', () => {
    const p = searchParams('tag:nlp in:title author:smith attention', allOpts);
    expect(p.get('q')).toBe('tag:nlp in:title author:smith attention');
    expect(p.get('fields')).toBeNull();
    expect(p.get('tag')).toBeNull();
  });

  it('lists only the selected engine', () => {
    const p = searchParams('x', { ...allOpts, semantic: false });
    expect(p.get('engines')).toBe('keyword');
  });

  it('keywordOnly overrides the engine selection', () => {
    const p = searchParams('x', allOpts, true);
    expect(p.get('engines')).toBe('keyword');
  });

  it('treats an empty engine selection the same as all (UI enforces at least one)', () => {
    const p = searchParams('x', { ...allOpts, keyword: false, semantic: false });
    expect(p.get('engines')).toBeNull();
  });
});
